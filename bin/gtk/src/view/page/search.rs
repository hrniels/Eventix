// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};

use chrono::Utc;
use gtk::{glib, prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::model::{
    View,
    search::{Cursor, Request},
};

/// Owns the search cursor, directional exhaustion, and deferred card selection.
#[derive(Default)]
pub(super) struct PageSearch {
    generation: Cell<u64>,
    pending: Cell<Option<bool>>,
    cursor: RefCell<Option<Cursor>>,
    exhausted_previous: Cell<bool>,
    exhausted_next: Cell<bool>,
    target: RefCell<Option<(u64, Cursor)>>,
}

impl PageSearch {
    pub(super) fn connect(&self, page: &CalendarPage) {
        let weak = page.downgrade();
        page.imp().search.connect_changed(move |_| {
            if let Some(page) = weak.upgrade() {
                page.imp().search_state.reset(&page);
            }
        });
        let weak = page.downgrade();
        page.imp().search.connect_activate(move |_| {
            if let Some(page) = weak.upgrade() {
                page.imp().search_state.start(&page, true);
            }
        });
        for (button, forward) in [
            (&*page.imp().search_previous, false),
            (&*page.imp().search_next, true),
        ] {
            let weak = page.downgrade();
            button.connect_clicked(move |_| {
                if let Some(page) = weak.upgrade() {
                    page.imp().search_state.start(&page, forward);
                }
            });
        }
    }

    pub(super) fn reset(&self, page: &CalendarPage) {
        self.cancel(page);
        self.cursor.take();
        self.exhausted_previous.set(false);
        self.exhausted_next.set(false);
        self.update_buttons(page);
    }

    pub(super) fn cancel(&self, page: &CalendarPage) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.pending.set(None);
        self.target.take();
        self.update_buttons(page);
    }

    fn update_buttons(&self, page: &CalendarPage) {
        for (button, exhausted) in [
            (&*page.imp().search_previous, self.exhausted_previous.get()),
            (&*page.imp().search_next, self.exhausted_next.get()),
        ] {
            button.set_sensitive(!exhausted);
        }
    }

    pub(super) fn start(&self, page: &CalendarPage, forward: bool) {
        if page.management_open()
            || page.imp().content.view.get() == View::List
            || self.pending.get().is_some()
        {
            return;
        }
        if if forward {
            self.exhausted_next.get()
        } else {
            self.exhausted_previous.get()
        } {
            page.error_bell();
            return;
        }
        let imp = page.imp();
        let session = imp.session.current.borrow();
        let Some(session) = session.as_ref() else {
            return;
        };
        self.cancel(page);
        self.pending.set(Some(forward));
        self.update_buttons(page);
        session.search(Request {
            generation: self.generation.get(),
            keywords: imp.search.text().to_string(),
            now: Utc::now().with_timezone(page.locale().timezone()),
            cursor: self.cursor.borrow().clone(),
            forward,
        });
    }

    pub(super) fn receive(
        &self,
        page: &CalendarPage,
        generation: u64,
        result: Result<Option<Cursor>, String>,
    ) {
        if generation != self.generation.get() {
            return;
        }
        let Some(forward) = self.pending.take() else {
            return;
        };
        match result {
            Ok(Some(cursor)) => {
                self.cursor.replace(Some(cursor.clone()));
                self.exhausted_previous.set(false);
                self.exhausted_next.set(false);
                page.show_content(Some(
                    cursor
                        .start
                        .with_timezone(page.locale().timezone())
                        .date_naive(),
                ));
                self.target
                    .replace(Some((page.imp().content.generation.get(), cursor)));
            }
            Ok(None) => {
                if forward {
                    self.exhausted_next.set(true)
                } else {
                    self.exhausted_previous.set(true)
                }
                page.error_bell();
            }
            Err(_) => {}
        }
        self.update_buttons(page);
    }

    pub(super) fn reveal_pending(&self) -> bool {
        self.target.borrow().is_some()
    }

    pub(super) fn reveal(&self, page: &CalendarPage, content_generation: u64) {
        let Some((generation, cursor)) = self.target.take() else {
            return;
        };
        if generation != content_generation {
            return;
        }
        let search_generation = self.generation.get();
        let weak = page.downgrade();
        let allocated = Cell::new(false);
        page.add_tick_callback(move |_, _| {
            let Some(page) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let imp = page.imp();
            if imp.content.generation.get() != generation
                || imp.search_state.generation.get() != search_generation
            {
                return glib::ControlFlow::Break;
            }
            if !allocated.replace(true) {
                return glib::ControlFlow::Continue;
            }
            let cards = if imp.content.view.get() == View::Month {
                imp.monthly.focus_cards()
            } else {
                imp.weekly.focus_cards()
            };
            if let Some(card) = cards
                .into_iter()
                .find(|card| card.event().is_some_and(|event| event.id == cursor.id))
            {
                crate::view::focus::grab_focus(card.upcast_ref());
            }
            glib::ControlFlow::Break
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use chrono::{Days, Utc};
    use eventix_ical::col::{CalDir, CalFile};
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};
    use gtk::{glib, prelude::*, subclass::prelude::*};

    use super::CalendarPage;
    use crate::{
        model::View,
        worker::{event::Event, session::Session},
    };

    fn state() -> (tempfile::TempDir, State, Vec<chrono::NaiveDate>) {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: tmp.path().display().to_string(),
        });
        let mut settings = CalendarSettings::default();
        settings.set_enabled(true);
        collection
            .all_calendars_mut()
            .insert("test".into(), settings);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        let id = Arc::new("test".to_string());
        let mut dir = CalDir::new_empty(id.clone(), tmp.path().into(), "Test".into(), false);
        let today = Utc::now().date_naive();
        let dates = vec![
            today - Days::new(45),
            today + Days::new(45),
            today + Days::new(80),
        ];
        for (index, date) in dates.iter().enumerate() {
            let times = if index == 0 {
                format!("DTSTART;VALUE=DATE:{}", date.format("%Y%m%d"))
            } else {
                format!(
                    "DTSTART:{}T233000Z\nDTEND:{}T234500Z",
                    date.format("%Y%m%d"),
                    date.format("%Y%m%d")
                )
            };
            let calendar = format!("BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:match-{index}\nSUMMARY:Needle\n{times}\nEND:VEVENT\nEND:VCALENDAR\n").parse().unwrap();
            dir.add_file(CalFile::new(
                id.clone(),
                tmp.path().join(format!("{index}.ics")),
                calendar,
            ))
            .unwrap();
        }
        state.store_mut().add(dir);
        (tmp, state, dates)
    }

    fn drain(page: &CalendarPage, responses: &async_channel::Receiver<Event>) {
        for _ in 0..40 {
            while let Ok(event) = responses.try_recv() {
                match event {
                    Event::SearchFinished(generation, result) => {
                        page.imp().search_state.receive(page, generation, result)
                    }
                    Event::ContentLoaded(response) => {
                        page.imp().content.receive_content(page, response)
                    }
                    _ => {}
                }
            }
            while glib::MainContext::default().iteration(false) {}
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_selected(page: &CalendarPage, window: &gtk::Window, uid: &str) {
        let cards = if page.imp().content.view.get() == View::Week {
            page.imp().weekly.focus_cards()
        } else {
            page.imp().monthly.focus_cards()
        };
        let card = cards
            .into_iter()
            .find(|card| card.event().is_some_and(|event| event.id.uid == uid))
            .unwrap();
        assert_eq!(RootExt::focus(window), Some(card.clone().upcast()));
        assert!(window.property::<bool>("focus-visible"));
        assert!(!page.popup_open());
        let mut ancestor = card.parent();
        while let Some(parent) = ancestor {
            if let Some(scroll) = parent.downcast_ref::<gtk::ScrolledWindow>() {
                let bounds = card.compute_bounds(scroll).unwrap();
                assert!(bounds.y() >= -1.0, "card is above viewport: {bounds:?}");
                assert!(
                    bounds.y() + bounds.height() <= scroll.height() as f32 + 1.0,
                    "card is below viewport: {bounds:?}"
                );
            }
            ancestor = parent.parent();
        }
    }

    fn press_search(window: &gtk::Window, previous: bool) {
        let controllers = window.observe_controllers();
        let keys = (0..controllers.n_items())
            .filter_map(|index| controllers.item(index))
            .filter_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .find(|keys| keys.propagation_phase() == gtk::PropagationPhase::Capture)
            .unwrap();
        let mut modifiers = gtk::gdk::ModifierType::CONTROL_MASK;
        if previous {
            modifiers |= gtk::gdk::ModifierType::SHIFT_MASK;
        }
        assert!(keys.emit_by_name::<bool>("key-pressed", &[&gtk::gdk::Key::g, &0u32, &modifiers],));
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn occurrence_search_disables_only_after_an_unsuccessful_search() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let (_tmp, state, dates) = state();
        let (session, responses) = Session::test_start(state);
        let app = gtk::Application::builder()
            .application_id("io.github.hrniels.Eventix.SearchNavigationTest")
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let (window, page) = crate::window::Window::test_new(&app);
        let window: gtk::Window = window.upcast();
        page.imp().session.current.replace(Some(session));
        window.set_default_size(1200, 650);
        window.present();
        let imp = page.imp();
        for view in [View::Week, View::Month] {
            imp.content.switch_view(&page, view);
            page.show_content(Some("1990-01-01".parse().unwrap()));
            imp.search.set_text("");
            imp.search.set_text("needle");
            drain(&page, &responses);
            imp.search.emit_activate();
            assert!(imp.search_previous.is_sensitive() && imp.search_next.is_sensitive());
            let pending = imp.search_state.generation.get();
            imp.search_next.emit_clicked();
            assert_eq!(
                imp.search_state.generation.get(),
                pending,
                "ignore duplicate requests"
            );
            drain(&page, &responses);
            assert_eq!(imp.content.view.get(), view);
            assert_eq!(imp.content.date.get(), Some(dates[1]));
            assert_selected(&page, &window, "match-1");
            assert!(imp.search_previous.is_sensitive());
            assert!(imp.search_next.is_sensitive());
            press_search(&window, false);
            drain(&page, &responses);
            assert_eq!(imp.content.date.get(), Some(dates[2]));
            assert_selected(&page, &window, "match-2");
            assert!(imp.search_next.is_sensitive());
            let generation = imp.content.generation.get();
            imp.search_next.emit_clicked();
            assert!(
                imp.search_next.is_sensitive(),
                "searching must not disable buttons"
            );
            drain(&page, &responses);
            assert_eq!(imp.content.generation.get(), generation);
            assert!(!imp.search_next.is_sensitive());
            assert!(imp.search_previous.is_sensitive());
            press_search(&window, true);
            drain(&page, &responses);
            assert_selected(&page, &window, "match-1");
            assert!(imp.search_next.is_sensitive());
            imp.search_previous.emit_clicked();
            drain(&page, &responses);
            assert_selected(&page, &window, "match-0");
            assert!(imp.search_previous.is_sensitive());
            imp.search_previous.emit_clicked();
            assert!(imp.search_previous.is_sensitive());
            drain(&page, &responses);
            assert!(!imp.search_previous.is_sensitive());
            assert!(imp.search_next.is_sensitive());

            imp.search.set_text("");
            imp.search.set_text("needle");
            imp.search.grab_focus();
            press_search(&window, true);
            drain(&page, &responses);
            assert_selected(&page, &window, "match-0");
            assert!(imp.search_previous.is_sensitive());
            assert!(imp.search_next.is_sensitive());

            imp.search.set_text("absent");
            assert!(imp.search_previous.is_sensitive() && imp.search_next.is_sensitive());
            let date = imp.content.date.get();
            imp.search.emit_activate();
            drain(&page, &responses);
            assert!(!imp.search_next.is_sensitive());
            assert!(imp.search_previous.is_sensitive());
            assert_eq!(imp.content.date.get(), date);
            assert_selected(&page, &window, "match-0");
            imp.search_previous.emit_clicked();
            drain(&page, &responses);
            assert!(!imp.search_previous.is_sensitive());

            imp.search.set_text("needle");
            imp.search.emit_activate();
            let stale = imp.search_state.generation.get();
            imp.search.set_text("changed before response");
            imp.search_state.receive(&page, stale, Ok(None));
            drain(&page, &responses);
            assert!(imp.search_previous.is_sensitive() && imp.search_next.is_sensitive());
            assert_eq!(imp.content.date.get(), date);
        }
        window.close();
        page.stop();
    }
}
