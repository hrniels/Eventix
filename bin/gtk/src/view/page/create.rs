// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, NaiveDateTime};
use gtk::{prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::view::creation_hint::is_creation_target;

#[derive(Clone, Copy)]
pub(super) struct EventSlot {
    pub start: NaiveDateTime,
    pub all_day: bool,
}

impl EventSlot {
    fn all_day(date: NaiveDate) -> Self {
        Self {
            start: date.and_time(chrono::NaiveTime::MIN),
            all_day: true,
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
