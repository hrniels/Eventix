// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use chrono::{Timelike, Utc};
use eventix_view::{Event, WeekSnapshot};
use gtk::{gdk, glib, graphene, prelude::*, subclass::prelude::*};

use crate::{event_card::EventCard, event_details::EventDetails, style};

pub const GUTTER: i32 = 64;
const ROW: i32 = 30;

struct Card {
    button: EventCard,
    event: Event,
    day: usize,
    row: usize,
}

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::Timeline)]
    pub struct Timeline {
        pub(super) week: RefCell<Option<Rc<WeekSnapshot>>>,
        pub(super) cards: RefCell<Vec<Card>>,
        #[property(get, set, construct_only)]
        pub(super) all_day: Cell<bool>,
        pub(super) popover: RefCell<Option<EventDetails>>,
        pub(super) peer: RefCell<Option<glib::WeakRef<super::Timeline>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Timeline {
        const NAME: &'static str = "EventixWeekTimeline";
        type Type = super::Timeline;
        type ParentType = gtk::Widget;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Timeline {
        fn constructed(&self) {
            self.parent_constructed();
            let timeline = self.obj();
            let gesture = gtk::GestureClick::new();
            gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
            let weak = timeline.downgrade();
            gesture.connect_pressed(move |_, _, x, y| {
                let Some(timeline) = weak.upgrade() else {
                    return;
                };
                let on_event = timeline.imp().cards.borrow().iter().any(|card| {
                    let allocation = card.button.allocation();
                    x >= f64::from(allocation.x())
                        && x < f64::from(allocation.x() + allocation.width())
                        && y >= f64::from(allocation.y())
                        && y < f64::from(allocation.y() + allocation.height())
                });
                if !on_event {
                    timeline.close_details();
                }
            });
            timeline.add_controller(gesture);
        }

        fn dispose(&self) {
            if let Some(popover) = self.popover.take() {
                popover.unparent();
            }
            for card in self.cards.take() {
                card.button.unparent();
            }
        }
    }

    impl WidgetImpl for Timeline {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let size = if orientation == gtk::Orientation::Horizontal {
                840
            } else if self.all_day.get() {
                self.cards
                    .borrow()
                    .iter()
                    .map(|c| c.row + 1)
                    .max()
                    .unwrap_or(1) as i32
                    * ROW
            } else {
                1440
            };
            (size, size, -1, -1)
        }

        fn size_allocate(&self, width: i32, _: i32, _: i32) {
            let day_width = f64::from(width - GUTTER) / 7.0;
            for card in self.cards.borrow().iter() {
                let event = &card.event;
                let (offset, span, count, y, height) = if self.all_day.get() {
                    (0, 1, 1, card.row as i32 * ROW, ROW - 2)
                } else {
                    let y = event.minute_start.min(1422) as i32;
                    let height = (event.minute_end as i32 - y).max(18).min(1440 - y);
                    (
                        event.columns.offset,
                        event.columns.span,
                        event.columns.count,
                        y,
                        height,
                    )
                };
                let x = f64::from(GUTTER)
                    + day_width * card.day as f64
                    + day_width * offset as f64 / count as f64;
                let right = f64::from(GUTTER)
                    + day_width * card.day as f64
                    + day_width * (offset + span) as f64 / count as f64;
                card.button.size_allocate(
                    &gtk::Allocation::new(
                        x.round() as i32 + 2,
                        y + 1,
                        ((right.round() - x.round()) as i32 - 4).max(1),
                        height - 2,
                    ),
                    -1,
                );
            }
            if let Some(popover) = self.popover.borrow().as_ref() {
                popover.present();
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let width = obj.width() as f32;
            let height = obj.height() as f32;
            let day_width = (width - GUTTER as f32) / 7.0;
            let line = style::color(&*obj, "ev_grid_line");
            let today = style::color(&*obj, "ev_today_background");
            let fold = style::color(&*obj, "ev_dst_fold");
            let gap = style::color(&*obj, "ev_dst_gap");
            let rect = |color: &gdk::RGBA, x, y, w, h| {
                snapshot.append_color(color, &graphene::Rect::new(x, y, w, h));
            };
            if let Some(week) = self.week.borrow().as_ref() {
                let now = Utc::now().with_timezone(week.locale.timezone());
                for (i, day) in week.days.iter().enumerate() {
                    let x = GUTTER as f32 + i as f32 * day_width;
                    if day.date == now.date_naive() {
                        rect(&today, x, 0.0, day_width, height);
                    }
                    if !self.all_day.get() {
                        for range in &day.dst {
                            rect(
                                if range.fold { &fold } else { &gap },
                                x,
                                range.start as f32,
                                day_width,
                                (range.end - range.start) as f32,
                            );
                            let text = week.locale.translate(if range.fold {
                                "Repeated hour"
                            } else {
                                "Skipped hour"
                            });
                            draw_text(
                                &obj,
                                snapshot,
                                text,
                                x + 4.0,
                                range.start as f32,
                                day_width - 8.0,
                            );
                        }
                    }
                }
            }
            for day in 0..=7 {
                rect(
                    &line,
                    GUTTER as f32 + day as f32 * day_width,
                    0.0,
                    1.0,
                    height,
                );
            }
            if !self.all_day.get() {
                for hour in 0..24 {
                    let y = hour as f32 * 60.0;
                    rect(&line, GUTTER as f32, y, width - GUTTER as f32, 1.0);
                    draw_text(&obj, snapshot, &format!("{hour:02}:00"), 7.0, y + 2.0, 52.0);
                }
            }
            for card in self.cards.borrow().iter() {
                obj.snapshot_child(&card.button, snapshot);
            }
            if !self.all_day.get()
                && let Some(week) = self.week.borrow().as_ref()
            {
                let now = Utc::now().with_timezone(week.locale.timezone());
                let day = (now.date_naive() - week.start).num_days();
                if (0..7).contains(&day) {
                    rect(
                        &style::color(&*obj, "ev_now"),
                        GUTTER as f32 + day as f32 * day_width,
                        (now.hour() * 60 + now.minute()) as f32,
                        day_width,
                        2.0,
                    );
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct Timeline(ObjectSubclass<imp::Timeline>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Timeline {
    pub(crate) fn event_cards(&self) -> Vec<(i64, EventCard)> {
        self.imp()
            .cards
            .borrow()
            .iter()
            .map(|card| (card.event.start.timestamp(), card.button.clone()))
            .collect()
    }

    pub fn set_peer(&self, peer: &Self) {
        self.imp().peer.replace(Some(peer.downgrade()));
    }

    pub fn set_week(&self, week: Rc<WeekSnapshot>) {
        self.close_details();
        let imp = self.imp();
        let mut old_cards = imp.cards.take();
        let mut cards = Vec::new();
        let mut previous: Option<EventCard> = None;
        for (day, data) in week.days.iter().enumerate() {
            let events = if imp.all_day.get() {
                &data.all_day
            } else {
                &data.timed
            };
            for (row, event) in events.iter().enumerate() {
                // Reuse live widgets to retain keyboard focus across local-data refreshes.
                let button =
                    if let Some(index) = old_cards.iter().position(|c| c.event.id == event.id) {
                        old_cards.remove(index).button
                    } else {
                        let button = EventCard::new();
                        let weak = self.downgrade();
                        button.connect_clicked(move |button| {
                            if let Some(timeline) = weak.upgrade() {
                                timeline.show_details(button);
                            }
                        });
                        button
                    };
                button.set_event(event, week.locale.as_ref(), imp.all_day.get());
                button.insert_after(self, previous.as_ref());
                previous = Some(button.clone());
                cards.push(Card {
                    button,
                    event: event.clone(),
                    day,
                    row,
                });
            }
        }
        for card in old_cards {
            card.button.unparent();
        }
        imp.cards.replace(cards);
        imp.week.replace(Some(week));
        self.queue_resize();
    }

    pub(crate) fn close_details(&self) {
        if let Some(popover) = self.imp().popover.take() {
            popover.popdown();
            popover.unparent();
        }
    }

    fn show_details(&self, button: &EventCard) {
        if let Some(peer) = self
            .imp()
            .peer
            .borrow()
            .as_ref()
            .and_then(|peer| peer.upgrade())
        {
            peer.close_details();
        }
        self.close_details();
        let Some(event) = button.event() else { return };
        let week = self.imp().week.borrow();
        let Some(week) = week.as_ref() else { return };
        let popover = EventDetails::new(&event, week.locale.as_ref());
        popover.set_parent(self);
        popover.set_pointing_to(Some(&button.allocation()));
        let weak = self.downgrade();
        popover.connect_closed(move |closed| {
            if let Some(timeline) = weak.upgrade() {
                let mut current = timeline.imp().popover.borrow_mut();
                if current.as_ref() == Some(closed) {
                    *current = None;
                }
                drop(current);
                closed.unparent();
            }
        });
        self.imp().popover.replace(Some(popover.clone()));
        popover.popup();
    }
}

fn draw_text(widget: &Timeline, snapshot: &gtk::Snapshot, text: &str, x: f32, y: f32, width: f32) {
    let layout = widget.create_pango_layout(Some(text));
    layout.set_width((width * gtk::pango::SCALE as f32) as i32);
    layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
    snapshot.save();
    snapshot.translate(&graphene::Point::new(x, y));
    snapshot.append_layout(&layout, &widget.color());
    snapshot.restore();
}
