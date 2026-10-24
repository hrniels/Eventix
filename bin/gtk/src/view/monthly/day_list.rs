// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{rc::Rc, sync::Arc};

use eventix_locale::Locale;
use eventix_view::MonthDay;
use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::view::{
    card::EventCard,
    popover::DetailsController,
    surface::{CardKey, Surface},
};

const GAP: i32 = 4;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct DayList {
        pub(super) surface: Rc<Surface>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DayList {
        const NAME: &'static str = "EventixMonthDayList";
        type Type = super::DayList;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for DayList {
        fn constructed(&self) {
            self.parent_constructed();
            self.surface.construct(self.obj().upcast_ref());
        }

        fn dispose(&self) {
            self.surface.dispose(self.obj().upcast_ref());
        }
    }

    impl WidgetImpl for DayList {
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
                self.surface
                    .cards
                    .borrow()
                    .iter()
                    .map(|card| card.button.measure(gtk::Orientation::Horizontal, -1).0)
                    .max()
                    .unwrap_or(1)
            } else {
                let count = self.surface.cards.borrow().len() as i32;
                count * (self.obj().card_height(for_size.max(1)) + GAP)
            };

            (size, size, -1, -1)
        }

        fn size_allocate(&self, width: i32, _: i32, _: i32) {
            let obj = self.obj();
            let height = obj.card_height(width);

            for (index, card) in self.surface.cards.borrow().iter().enumerate() {
                card.button.size_allocate(
                    &gtk::Allocation::new(0, index as i32 * (height + GAP), width, height),
                    -1,
                );
            }

            self.surface.details.borrow().allocate(obj.upcast_ref());
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.surface
                .snapshot_cards(self.obj().upcast_ref(), snapshot);
        }
    }
}

glib::wrapper! {
    pub struct DayList(ObjectSubclass<imp::DayList>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DayList {
    pub(crate) fn set_details(&self, details: Rc<DetailsController>) {
        self.imp().surface.details.replace(details);
    }

    pub(crate) fn event_cards(&self) -> Vec<(i64, EventCard)> {
        self.imp().surface.event_cards()
    }

    fn card_height(&self, width: i32) -> i32 {
        self.imp()
            .surface
            .cards
            .borrow()
            .iter()
            .map(|card| {
                let minimum = card.button.measure(gtk::Orientation::Horizontal, -1).0;
                card.button
                    .measure(gtk::Orientation::Vertical, width.max(minimum))
                    .1
            })
            .max()
            .unwrap_or(26)
            .max(26)
    }

    pub(crate) fn set_day(&self, day: &MonthDay, locale: Arc<dyn Locale + Send + Sync>) {
        let surface = &self.imp().surface;
        let keys = day
            .events
            .iter()
            .map(|event| CardKey::Timed(event.id()))
            .collect();
        surface.reconcile(self.upcast_ref(), locale.clone(), keys);

        for (card, event) in surface.cards.borrow().iter().zip(&day.events) {
            card.button.set_monthly(event, locale.as_ref());
        }

        self.queue_resize();
    }
}
