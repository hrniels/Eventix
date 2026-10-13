// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A shared scrolling surface for one month's week of spans and timed day lists.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use eventix_locale::Locale;
use eventix_view::{AllDaySpan, MonthDay};
use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::view::{
    card::EventCard,
    geometry::{GUTTER, day_x},
    popover::DetailsController,
    surface::{CardKey, Surface},
};

use super::day_list::DayList;

const ROW: i32 = 30;
const GAP: i32 = 4;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WeekContent {
        pub(super) surface: Rc<Surface>,
        pub(super) lists: RefCell<Vec<DayList>>,
        pub(super) spans: RefCell<Vec<AllDaySpan>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WeekContent {
        const NAME: &'static str = "EventixMonthWeekContent";
        type Type = super::WeekContent;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for WeekContent {
        fn constructed(&self) {
            self.parent_constructed();
            self.surface.construct(self.obj().upcast_ref());
            for _ in 0..7 {
                let list: DayList = glib::Object::new();
                list.set_parent(&*self.obj());
                self.lists.borrow_mut().push(list);
            }
        }

        fn dispose(&self) {
            for list in self.lists.take() {
                list.unparent();
            }
            self.surface.dispose(self.obj().upcast_ref());
        }
    }

    impl WidgetImpl for WeekContent {
        fn map(&self) {
            self.parent_map();
            self.surface.map(self.obj().upcast_ref());
        }

        fn unmap(&self) {
            self.surface.unmap();
            self.parent_unmap();
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let size = if orientation == gtk::Orientation::Horizontal {
                self.lists
                    .borrow()
                    .iter()
                    .map(|list| list.measure(orientation, -1).0 + 2 * GAP)
                    .max()
                    .unwrap_or(1)
                    * 7
            } else {
                let width = for_size.max(7);
                let timed = self
                    .lists
                    .borrow()
                    .iter()
                    .enumerate()
                    .map(|(day, list)| {
                        let w = self.obj().column_x(width, day + 1)
                            - self.obj().column_x(width, day)
                            - 2 * GAP;
                        let minimum = list.measure(gtk::Orientation::Horizontal, -1).0;
                        list.measure(orientation, w.max(minimum)).1
                    })
                    .max()
                    .unwrap_or(0);
                (self.obj().span_height() + timed + GAP).max(1)
            };
            (size, size, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _: i32) {
            let obj = self.obj();
            for (card, span) in self
                .surface
                .cards
                .borrow()
                .iter()
                .zip(self.spans.borrow().iter())
            {
                let left = obj.column_x(width, span.start_day);
                let right = obj.column_x(width, span.end_day);
                card.button.measure(gtk::Orientation::Horizontal, -1);
                card.button
                    .measure(gtk::Orientation::Vertical, (right - left - 2 * GAP).max(1));
                card.button.size_allocate(
                    &gtk::Allocation::new(
                        left + GAP,
                        span.row as i32 * ROW + 1,
                        (right - left - 2 * GAP).max(1),
                        ROW - GAP,
                    ),
                    -1,
                );
            }
            // Reserve the same span height in every column so timed lists stay aligned.
            let top = obj.span_height();
            for (day, list) in self.lists.borrow().iter().enumerate() {
                let left = obj.column_x(width, day);
                let right = obj.column_x(width, day + 1);
                let w = (right - left - 2 * GAP).max(1);
                list.measure(gtk::Orientation::Horizontal, -1);
                list.measure(gtk::Orientation::Vertical, w);
                list.size_allocate(
                    &gtk::Allocation::new(left + GAP, top, w, (height - top - GAP).max(1)),
                    -1,
                );
            }
            self.surface.details.borrow().allocate(obj.upcast_ref());
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            self.surface.snapshot_cards(obj.upcast_ref(), snapshot);
            for list in self.lists.borrow().iter() {
                obj.snapshot_child(list, snapshot);
            }
        }
    }
}

glib::wrapper! {
    pub struct WeekContent(ObjectSubclass<imp::WeekContent>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl WeekContent {
    fn column_x(&self, width: i32, day: usize) -> i32 {
        day_x(width + GUTTER, day as f64).round() as i32 - GUTTER
    }

    fn span_height(&self) -> i32 {
        self.imp()
            .spans
            .borrow()
            .iter()
            .map(|span| span.row + 1)
            .max()
            .unwrap_or(0) as i32
            * ROW
    }

    pub(crate) fn set_details(&self, details: Rc<DetailsController>) {
        self.imp().surface.details.replace(details.clone());
        for list in self.imp().lists.borrow().iter() {
            list.set_details(details.clone());
        }
    }

    pub(crate) fn set_week(
        &self,
        days: &[MonthDay],
        spans: &[AllDaySpan],
        locale: Arc<dyn Locale + Send + Sync>,
    ) {
        let surface = &self.imp().surface;
        surface.reconcile(
            self.upcast_ref(),
            locale.clone(),
            spans
                .iter()
                .map(|span| CardKey::Span(span.event.id.clone()))
                .collect(),
        );
        for (card, span) in surface.cards.borrow().iter().zip(spans) {
            card.button.set_span(span, locale.as_ref());
        }
        for (list, day) in self.imp().lists.borrow().iter().zip(days) {
            list.set_day(day, locale.clone());
        }
        self.imp().spans.replace(spans.to_vec());
        self.queue_resize();
    }

    pub(crate) fn event_cards(&self) -> Vec<(i64, EventCard)> {
        let mut cards = self.imp().surface.event_cards();
        cards.extend(
            self.imp()
                .lists
                .borrow()
                .iter()
                .flat_map(DayList::event_cards),
        );
        cards
    }
}
