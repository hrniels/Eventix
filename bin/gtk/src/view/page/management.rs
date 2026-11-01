// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, OnceCell};

use eventix_view::calendars::Snapshot;
use gtk::{prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::model::activity::{Activity, Phase};
use crate::model::management::Request;
use crate::view::calendars::CalendarsView;

#[derive(Default)]
pub(super) struct PageManagement {
    pub(super) visible: Cell<bool>,
    generation: Cell<u64>,
    view: OnceCell<CalendarsView>,
}

impl PageManagement {
    pub(super) fn connect(&self, page: &CalendarPage) {
        let weak = page.downgrade();
        page.imp().calendars_button.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.imp().management.open(&page);
            }
        });
    }

    fn open(&self, page: &CalendarPage) {
        if page.editor_open() || self.visible.replace(true) {
            return;
        }
        page.dismiss_popovers();
        let imp = page.imp();
        imp.picker.popdown();
        let view = self.view.get_or_init(|| {
            let view = CalendarsView::new(page.locale().as_ref());
            let weak = page.downgrade();
            view.connect_action(move |request| {
                if let Some(page) = weak.upgrade()
                    && let Some(session) = page.imp().session.current.borrow().as_ref()
                {
                    session.manage_calendar(request);
                }
            });
            if let Some(group) = imp.header_group.borrow().as_ref() {
                group.add_widget(&view.header);
            }
            imp.page_stack.add_child(&view.widget);
            view
        });
        imp.page_stack.set_visible_child(&view.widget);
        imp.search.set_visible(false);
        imp.week_view.set_active(false);
        imp.month_view.set_active(false);
        imp.list_view.set_active(false);
        if let Some(window) = page.root().and_downcast::<gtk::Window>() {
            window.set_title(Some(&format!(
                "Eventix — {}",
                page.locale().translate("Calendars")
            )));
        }
        self.refresh(page);
    }

    pub(super) fn leave(&self, page: &CalendarPage) -> bool {
        if !self.visible.replace(false) {
            return false;
        }
        self.generation.set(self.generation.get().wrapping_add(1));
        page.imp().page_stack.set_visible_child(&*page.imp().views);
        true
    }

    pub(super) fn refresh(&self, page: &CalendarPage) {
        if self.visible.get()
            && let Some(session) = page.imp().session.current.borrow().as_ref()
        {
            let generation = self.generation.get().wrapping_add(1);
            self.generation.set(generation);
            session.load_management(generation);
        }
    }

    pub(super) fn finished(
        &self,
        page: &CalendarPage,
        request: &Request,
        result: &Result<(), String>,
    ) {
        if let Some(view) = self.view.get() {
            view.finish(request, result.is_ok());
            if let Err(error) = result {
                view.show_error(request, error);
            }
        }
        self.refresh(page);
    }

    pub(super) fn receive(
        &self,
        page: &CalendarPage,
        generation: u64,
        result: Result<Snapshot, String>,
    ) {
        if !self.visible.get() || generation != self.generation.get() {
            return;
        }
        if let Ok(snapshot) = result {
            for warning in &snapshot.warnings {
                let activity = Activity::new(0, "Loading calendar")
                    .finish(Phase::Failed, Some(warning.clone()));
                page.imp()
                    .status
                    .record(&activity, snapshot.locale.as_ref());
            }
            page.imp().locale.replace(Some(snapshot.locale.clone()));
            if let Some(window) = page.root().and_downcast::<gtk::Window>() {
                window.set_title(Some(&format!(
                    "Eventix — {}",
                    snapshot.locale.translate("Calendars")
                )));
            }
            if let Some(view) = self.view.get() {
                view.set_snapshot(snapshot);
            }
        } else if let Some(view) = self.view.get() {
            view.load_failed(page.locale().as_ref());
        }
    }
}

#[cfg(test)]
mod tests {
    use gtk::{prelude::*, subclass::prelude::*};

    use super::Snapshot;
    use crate::model::session::View;

    fn contains_text(widget: &gtk::Widget, text: &str) -> bool {
        if widget
            .downcast_ref::<gtk::Label>()
            .is_some_and(|label| label.text() == text)
        {
            return true;
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if contains_text(&widget, text) {
                return true;
            }
            child = widget.next_sibling();
        }
        false
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn calendars_management_navigation_preserves_dates_filters_and_rejects_stale_results() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("org.eventix.CalendarsTest")
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let (window, page) = crate::window::Window::test_new(&app);
        window.present();
        let imp = page.imp();
        let date = "2026-10-25".parse().unwrap();
        page.show_content(Some(date));
        imp.list.get().unwrap().set_keywords("meeting");
        let drain = || {
            for _ in 0..10 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        for view in [View::Week, View::Month, View::List] {
            imp.content.switch_view(&page, view);
            drain();
            imp.calendars_button.emit_clicked();
            drain();
            assert!(page.management_open());
            assert_eq!(
                imp.management
                    .view
                    .get()
                    .unwrap()
                    .header
                    .measure(gtk::Orientation::Vertical, -1),
                imp.header.measure(gtk::Orientation::Vertical, -1),
                "management and calendar headers share the same height"
            );
            assert!(!imp.sidebar.is_mapped());
            assert!(!imp.search.is_mapped());
            assert!(imp.status.is_mapped());
            assert_eq!(window.title().as_deref(), Some("Eventix — Calendars"));
            let generation = imp.content.generation.get();
            for action in [
                "previous",
                "next",
                "today",
                "new-event",
                "new-task",
                "quick-add-task",
                "search",
            ] {
                gtk::prelude::ActionGroupExt::activate_action(&window, action, None);
            }
            assert_eq!(imp.content.generation.get(), generation);
            assert!(!page.editor_open());
            assert_eq!(imp.content.date.get(), Some(date));
            assert_eq!(imp.list.get().unwrap().filter().keywords, "meeting");
            let stale = imp.management.generation.get();
            imp.management
                .receive(&page, stale, Err("unavailable".into()));
            imp.content.switch_view(&page, view);
            drain();
            assert!(!page.management_open());
            assert!(imp.sidebar.is_mapped());
            assert_eq!(imp.content.view.get(), view);
            assert_eq!(imp.content.date.get(), Some(date));
            imp.calendars_button.emit_clicked();
            imp.management.receive(
                &page,
                stale,
                Ok(Snapshot {
                    collections: vec![],
                    locale: eventix_locale::default(),
                    warnings: vec![],
                }),
            );
            assert_eq!(
                imp.management.view.get().unwrap().widget,
                imp.page_stack
                    .visible_child()
                    .unwrap()
                    .downcast::<gtk::Frame>()
                    .unwrap()
            );
            let management = &imp.management.view.get().unwrap().widget;
            assert!(contains_text(
                management.upcast_ref(),
                "Could not load local calendars"
            ));
            assert!(!contains_text(
                management.upcast_ref(),
                "No calendars found"
            ));
            imp.management.receive(
                &page,
                imp.management.generation.get(),
                Ok(Snapshot {
                    collections: vec![],
                    locale: eventix_locale::default(),
                    warnings: vec![],
                }),
            );
            assert!(contains_text(management.upcast_ref(), "No calendars found"));
            imp.content.switch_view(&page, view);
        }
        window.close();
    }
}
