// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use chrono::Utc;
use eventix_view::WeekSnapshot;
use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::view::geometry::{MIN_WIDTH, day_x, rect};
use crate::{
    style,
    view::{
        card::EventCard,
        popover::DetailsController,
        surface::{CardKey, Surface},
    },
};

const ROW: i32 = 30;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AllDayStrip {
        pub(super) surface: Rc<Surface>,
        pub week: RefCell<Option<Rc<WeekSnapshot>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AllDayStrip {
        const NAME: &'static str = "EventixAllDayStrip";
        type Type = super::AllDayStrip;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for AllDayStrip {
        fn constructed(&self) {
            self.parent_constructed();
            self.surface.construct(self.obj().upcast_ref());
        }

        fn dispose(&self) {
            self.surface.dispose(self.obj().upcast_ref());
        }
    }

    impl WidgetImpl for AllDayStrip {
        fn map(&self) {
            self.parent_map();
            self.surface.map(self.obj().upcast_ref());
        }

        fn unmap(&self) {
            self.surface.unmap();
            self.parent_unmap();
        }

        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let size = if orientation == gtk::Orientation::Horizontal {
                MIN_WIDTH
            } else {
                self.week
                    .borrow()
                    .as_ref()
                    .and_then(|week| week.all_day.iter().map(|span| span.row + 1).max())
                    .unwrap_or(1) as i32
                    * ROW
            };
            (size, size, -1, -1)
        }

        fn size_allocate(&self, width: i32, _: i32, _: i32) {
            if let Some(week) = self.week.borrow().as_ref() {
                for (card, span) in self.surface.cards.borrow().iter().zip(&week.all_day) {
                    let left = day_x(width, span.start_day as f64).round() as i32;
                    let right = day_x(width, span.end_day as f64).round() as i32;
                    card.button.size_allocate(
                        &gtk::Allocation::new(
                            left + 2,
                            span.row as i32 * ROW + 1,
                            (right - left - 4).max(1),
                            ROW - 4,
                        ),
                        -1,
                    );
                }
            }
            self.surface
                .details
                .borrow()
                .allocate(self.obj().upcast_ref());
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let height = obj.height() as f32;
            let line = style::color(&*obj, "ev_grid_line");
            if let Some(week) = self.week.borrow().as_ref() {
                let today = Utc::now()
                    .with_timezone(week.locale.timezone())
                    .date_naive();
                for (i, day) in week.days.iter().enumerate() {
                    if day.date == today {
                        let x = day_x(obj.width(), i as f64) as f32;
                        let right = day_x(obj.width(), (i + 1) as f64) as f32;
                        rect(
                            snapshot,
                            &style::color(&*obj, "ev_today_background"),
                            x,
                            0.0,
                            right - x,
                            height,
                        );
                    }
                }
            }
            for day in 0..=7 {
                rect(
                    snapshot,
                    &line,
                    day_x(obj.width(), day as f64) as f32,
                    0.0,
                    1.0,
                    height,
                );
            }
            self.surface.snapshot_cards(obj.upcast_ref(), snapshot);
        }
    }
}

glib::wrapper! {
    pub struct AllDayStrip(ObjectSubclass<imp::AllDayStrip>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AllDayStrip {
    pub(crate) fn set_details(&self, details: Rc<DetailsController>) {
        self.imp().surface.details.replace(details);
    }

    pub(crate) fn event_cards(&self) -> Vec<(i64, EventCard)> {
        self.imp().surface.event_cards()
    }

    pub fn set_week(&self, week: Rc<WeekSnapshot>) {
        let surface = &self.imp().surface;
        let keys = week
            .all_day
            .iter()
            .map(|span| CardKey::Span(span.event.id.clone()))
            .collect();
        surface.reconcile(self.upcast_ref(), week.locale.clone(), keys);
        for (card, span) in surface.cards.borrow().iter().zip(&week.all_day) {
            card.button.set_span(span, week.locale.as_ref());
        }
        self.imp().week.replace(Some(week));
        self.queue_resize();
    }
}
