// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, NaiveDateTime};
use gtk::{prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::view::creation_hint::is_creation_target;
use crate::view::selection::{self, SelectionRange};

#[derive(Clone, Copy)]
pub(super) struct EventSlot {
    pub start: NaiveDateTime,
    pub all_day: bool,
    /// The last selected date for all-day events, otherwise the exclusive timed end.
    pub end: Option<NaiveDateTime>,
}

impl EventSlot {
    fn all_day(date: NaiveDate) -> Self {
        Self {
            start: date.and_time(chrono::NaiveTime::MIN),
            all_day: true,
            end: None,
        }
    }
}

impl CalendarPage {
    pub(super) fn connect_event_creation(&self) {
        let imp = self.imp();
        self.connect_slot_creation(&*imp.monthly, |page, x, y| {
            page.imp().monthly.drop_date(x, y).map(EventSlot::all_day)
        });

        self.connect_slot_creation(&*imp.weekly.imp().all_day, |page, x, y| {
            page.imp()
                .weekly
                .imp()
                .all_day
                .date_at(x, y)
                .map(EventSlot::all_day)
        });

        self.connect_slot_creation(&*imp.weekly.imp().timed, |page, x, y| {
            let timed = &page.imp().weekly.imp().timed;
            // The grid uses one vertical pixel per minute; creation selects whole hours.
            let start = timed.drop_start(x, (y / 60.0).floor() * 60.0, 0)?;

            Some(EventSlot {
                start,
                all_day: false,
                end: None,
            })
        });

        self.connect_slot_creation(&*imp.weekly.imp().headers, |page, x, _| {
            let weekly = page.imp().weekly.imp();
            let width = f64::from(weekly.headers.width());
            if width <= 0.0 || !(0.0..width).contains(&x) {
                return None;
            }

            let col = (x * 7.0 / width).floor() as usize;
            let week = weekly.timed.imp().week.borrow();

            Some(EventSlot::all_day(week.as_ref()?.days.get(col)?.date))
        });

        selection::install(
            &*imp.weekly.imp().timed,
            |grid, x, y, clamp| grid.selection_point(x, y, clamp),
            |grid, range| grid.set_selection_preview(range),
            |grid| grid.clear_drop_preview(),
        );
        selection::install(
            &*imp.monthly,
            |grid, x, y, clamp| grid.selection_point(x, y, clamp),
            |grid, range| grid.set_selection_preview(range),
            crate::actions::items::move_event::MoveTarget::clear_drop_preview,
        );
        selection::install(
            &*imp.weekly.imp().all_day,
            |grid, x, y, clamp| grid.selection_point(x, y, clamp),
            |grid, range| grid.set_selection_preview(range),
            crate::actions::items::move_event::MoveTarget::clear_drop_preview,
        );
    }

    pub(crate) fn can_create_event(&self) -> bool {
        self.can_edit() && !self.imp().editor.editor_open()
    }

    pub(super) fn cancel_event_selection(&self) {
        selection::cancel(&*self.imp().monthly);
        selection::cancel(&*self.imp().weekly.imp().timed);
        selection::cancel(&*self.imp().weekly.imp().all_day);
    }

    pub(crate) fn create_event_range(&self, range: SelectionRange) {
        let slot = match range {
            SelectionRange::Timed(_, _) => {
                let Some((start, end)) = range.timed_bounds(self.locale().timezone()) else {
                    return;
                };
                EventSlot {
                    start: start.naive_local(),
                    end: Some(end.naive_local()),
                    all_day: false,
                }
            }
            SelectionRange::AllDay(start, end) => EventSlot {
                start: start.and_time(chrono::NaiveTime::MIN),
                end: Some(end.and_time(chrono::NaiveTime::MIN)),
                all_day: true,
            },
        };
        self.imp().editor.new_event_at(self, slot);
    }

    fn connect_slot_creation(
        &self,
        widget: &impl IsA<gtk::Widget>,
        slot_at: impl Fn(&Self, f64, f64) -> Option<EventSlot> + 'static,
    ) {
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_PRIMARY);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);

        let weak = self.downgrade();
        click.connect_pressed(move |gesture, count, x, y| {
            if count != 2 {
                return;
            }

            let Some(widget) = gesture.widget() else {
                return;
            };

            if !is_creation_target(&widget, x, y) {
                return;
            }

            if let Some(page) = weak.upgrade()
                && let Some(slot) = slot_at(&page, x, y)
            {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                page.imp().editor.new_event_at(&page, slot);
            }
        });

        widget.add_controller(click);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, NaiveDate, NaiveDateTime};
    use eventix_view::{Day, MonthDay, MonthSnapshot, WeekSnapshot, editor::EditorCalendar};
    use gtk::{glib, prelude::*, subclass::prelude::*};

    use super::CalendarPage;
    use crate::model::session::{CalendarSession, Event};

    fn drag(widget: &impl IsA<gtk::Widget>) -> gtk::GestureDrag {
        widget
            .observe_controllers()
            .snapshot()
            .into_iter()
            .find_map(|controller| controller.downcast::<gtk::GestureDrag>().ok())
            .unwrap()
    }

    fn entries(widget: &gtk::Widget) -> Vec<gtk::Entry> {
        let mut result = Vec::new();
        if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
            result.push(entry.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            result.extend(entries(&widget));
            child = widget.next_sibling();
        }
        result
    }

    fn assert_editor_range(
        page: &CalendarPage,
        responses: &async_channel::Receiver<Event>,
        start: NaiveDateTime,
        end: NaiveDateTime,
        all_day: bool,
    ) {
        assert!(page.imp().editor.editor_open());
        glib::MainContext::default().block_on(async {
            loop {
                if let Event::EditorLoaded(id, result) = responses.recv().await.unwrap() {
                    let mut options = result.unwrap();
                    options.calendars.push(EditorCalendar {
                        id: "test".into(),
                        name: "Test".into(),
                        background: "#27c281".into(),
                        personal: false,
                    });
                    page.imp().editor.editor_loaded(page, id, Ok(options));
                    break;
                }
            }
        });
        let editor = page.imp().editor.current.borrow().clone().unwrap();
        // The summary is the first entry in the editor form.
        entries(&editor.popup_panel())[0].set_text("Drag selection");
        let command = editor.command().unwrap();
        let context = eventix_ical::objects::DateContext::system();
        let tz = *page.locale().timezone();
        for (actual, expected) in [
            (command.start.as_ref().unwrap(), start),
            (command.end.as_ref().unwrap(), end),
        ] {
            assert_eq!(context.date(actual).start_in(&tz).naive_local(), expected);
            assert_eq!(
                matches!(actual, eventix_ical::objects::CalDate::Date(..)),
                all_day
            );
        }
        page.close_editor();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn background_drag_opens_range_editor_and_preserves_click_creation() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let state = eventix_state::State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        let (session, responses) = CalendarSession::test_start(state);
        let page: CalendarPage = glib::Object::new();
        page.imp().session.current.replace(Some(session));
        let start: NaiveDate = "2026-10-19".parse().unwrap();
        let week = WeekSnapshot {
            start,
            days: (0..7)
                .map(|offset| Day {
                    date: start + Duration::days(offset),
                    timed: vec![],
                    dst: vec![],
                })
                .collect(),
            all_day: vec![],
            locale: eventix_locale::default(),
        };
        page.imp().content.show_week(&page, week);
        page.imp().content.loading.set(false);
        let window = gtk::Window::builder()
            .child(&page)
            .default_width(1200)
            .default_height(800)
            .build();
        window.present();
        for _ in 0..20 {
            while glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let timed = &*page.imp().weekly.imp().timed;
        let x = crate::view::geometry::day_x(timed.width(), 0.5);
        use crate::view::selection::SelectionPoint;
        assert_eq!(
            timed.selection_point(x, 1.0, false),
            Some(SelectionPoint::Timed(start.and_hms_opt(0, 0, 0).unwrap()))
        );
        assert_eq!(
            timed.selection_point(x, 1439.0, false),
            Some(SelectionPoint::Timed(start.and_hms_opt(23, 45, 0).unwrap()))
        );
        assert_eq!(
            timed.selection_point(x, 1440.0, true),
            Some(SelectionPoint::Timed(
                start.succ_opt().unwrap().and_hms_opt(0, 0, 0).unwrap()
            ))
        );
        let gesture = drag(timed);

        // A click and a cancelled gesture must not request an editor.
        gesture.emit_by_name::<()>("drag-begin", &[&x, &570.0_f64]);
        gesture.emit_by_name::<()>("drag-end", &[&0.0_f64, &0.0_f64]);
        assert!(!page.imp().editor.editor_open());
        gesture.emit_by_name::<()>("drag-begin", &[&x, &570.0_f64]);
        gesture.emit_by_name::<()>("drag-update", &[&0.0_f64, &90.0_f64]);
        gesture.emit_by_name::<()>("cancel", &[&None::<gtk::gdk::EventSequence>]);
        gesture.emit_by_name::<()>("drag-end", &[&0.0_f64, &90.0_f64]);
        assert!(!page.imp().editor.editor_open());

        for days in [0, 2] {
            let dx = crate::view::geometry::day_x(timed.width(), days as f64 + 0.5) - x;
            gesture.emit_by_name::<()>("drag-begin", &[&x, &570.0_f64]);
            gesture.emit_by_name::<()>("drag-update", &[&dx, &90.0_f64]);
            assert!(!page.imp().editor.editor_open(), "dragging only previews");
            gesture.emit_by_name::<()>("drag-end", &[&dx, &90.0_f64]);
            assert_editor_range(
                &page,
                &responses,
                start.and_hms_opt(9, 30, 0).unwrap(),
                (start + Duration::days(days))
                    .and_hms_opt(11, 0, 0)
                    .unwrap(),
                false,
            );
        }

        // Double-click creation still uses the existing whole-hour, one-hour default.
        let click = timed
            .observe_controllers()
            .snapshot()
            .into_iter()
            .find_map(|controller| controller.downcast::<gtk::GestureClick>().ok())
            .unwrap();
        click.emit_by_name::<()>("pressed", &[&2_i32, &x, &570.0_f64]);
        assert_editor_range(
            &page,
            &responses,
            start.and_hms_opt(9, 0, 0).unwrap(),
            start.and_hms_opt(10, 0, 0).unwrap(),
            false,
        );

        let strip = &*page.imp().weekly.imp().all_day;
        let gesture = drag(strip);
        let x = crate::view::geometry::day_x(strip.width(), 3.5);
        let dx = crate::view::geometry::day_x(strip.width(), 1.5) - x;
        gesture.emit_by_name::<()>("drag-begin", &[&x, &10.0_f64]);
        gesture.emit_by_name::<()>("drag-update", &[&dx, &0.0_f64]);
        gesture.emit_by_name::<()>("drag-end", &[&dx, &0.0_f64]);
        assert_editor_range(
            &page,
            &responses,
            (start + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap(),
            (start + Duration::days(4)).and_hms_opt(0, 0, 0).unwrap(),
            true,
        );

        let (month_start, month_end) = eventix_view::month_range(start).unwrap();
        page.imp().month_view.set_active(true);
        page.imp().content.show_month(
            &page,
            MonthSnapshot {
                month: "2026-10-01".parse().unwrap(),
                start: month_start,
                days: (0..(month_end - month_start).num_days())
                    .map(|offset| MonthDay {
                        date: month_start + Duration::days(offset),
                        current_month: true,
                        show_month: false,
                        events: vec![],
                    })
                    .collect(),
                spans: vec![vec![]; ((month_end - month_start).num_days() / 7) as usize],
                locale: eventix_locale::default(),
            },
        );
        for _ in 0..20 {
            while glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let month = &*page.imp().monthly;
        page.imp().content.loading.set(false);
        let date_point = |date: NaiveDate| {
            for col in 0..7 {
                let x = crate::view::geometry::day_x(month.width(), col as f64 + 0.5);
                for y in 0..month.height() {
                    if month.drop_date(x, f64::from(y)) == Some(date) {
                        return (x, f64::from(y) + 10.0);
                    }
                }
            }
            panic!("date missing from month grid");
        };
        let a = date_point("2026-10-25".parse().unwrap());
        let b = date_point("2026-11-01".parse().unwrap());
        for (a, b) in [(a, b), (b, a)] {
            let gesture = drag(month);
            gesture.emit_by_name::<()>("drag-begin", &[&a.0, &a.1]);
            gesture.emit_by_name::<()>("drag-update", &[&(b.0 - a.0), &(b.1 - a.1)]);
            gesture.emit_by_name::<()>("drag-end", &[&(b.0 - a.0), &(b.1 - a.1)]);
            assert_editor_range(
                &page,
                &responses,
                "2026-10-25T00:00:00".parse().unwrap(),
                "2026-11-02T00:00:00".parse().unwrap(),
                true,
            );
        }
        window.close();
    }
}
