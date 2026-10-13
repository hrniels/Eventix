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
const DETAILS_PLACEMENT_MARGIN: i32 = 16;
const DETAILS_PLACEMENT_HYSTERESIS: i32 = 64;

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
        pub(super) anchor: RefCell<Option<glib::WeakRef<EventCard>>>,
        pub(super) details_minimum_height: Cell<i32>,
        pub(super) details_width: Cell<i32>,
        pub(super) details_centered: Cell<bool>,
        pub(super) details_force_centered: Cell<bool>,
        pub(super) scroll_handlers: RefCell<Vec<(gtk::Adjustment, glib::SignalHandlerId)>>,
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
            for (adjustment, handler) in self.scroll_handlers.take() {
                adjustment.disconnect(handler);
            }
            if let Some(popover) = self.popover.take() {
                popover.popdown();
                if popover.parent().is_some() {
                    popover.unparent();
                }
            }
            for card in self.cards.take() {
                card.button.unparent();
            }
            // Closed popovers can still be parented while their idle cleanup is pending.
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Timeline {
        fn map(&self) {
            self.parent_map();
            let obj = self.obj();
            let mut ancestor = obj.parent();
            while let Some(widget) = ancestor {
                if let Some(scroll) = widget.downcast_ref::<gtk::ScrolledWindow>() {
                    for adjustment in [scroll.hadjustment(), scroll.vadjustment()] {
                        let weak = obj.downgrade();
                        let handler = adjustment.connect_value_changed(move |_| {
                            if let Some(timeline) = weak.upgrade() {
                                timeline.queue_allocate();
                            }
                        });
                        self.scroll_handlers
                            .borrow_mut()
                            .push((adjustment.clone(), handler));
                        let weak = obj.downgrade();
                        let handler = adjustment.connect_changed(move |_| {
                            if let Some(timeline) = weak.upgrade() {
                                timeline.queue_allocate();
                            }
                        });
                        self.scroll_handlers
                            .borrow_mut()
                            .push((adjustment, handler));
                    }
                }
                ancestor = widget.parent();
            }
        }

        fn unmap(&self) {
            for (adjustment, handler) in self.scroll_handlers.take() {
                adjustment.disconnect(handler);
            }
            self.parent_unmap();
        }

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
            let popover = self.popover.borrow().clone();
            if let Some(popover) = popover {
                self.obj().place_details(&popover);
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
        self.imp().anchor.take();
        if let Some(popover) = self.imp().popover.take() {
            popover.popdown();
            Self::defer_unparent(&popover);
        }
    }

    fn defer_unparent(popover: &EventDetails) {
        // GTK can continue using the popover after emitting `closed`, including on failed
        // presentation. Keep it alive and parented until that GTK call has returned.
        let popover = popover.clone();
        glib::idle_add_local_once(move || {
            if popover.parent().is_some() {
                popover.unparent();
            }
        });
    }

    /// Centers the popup when its event is off-screen or anchored placement cannot fit.
    fn place_details(&self, popover: &EventDetails) {
        let Some(button) = self
            .imp()
            .anchor
            .borrow()
            .as_ref()
            .and_then(|a| a.upgrade())
        else {
            return;
        };
        let Some(root) = self.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let bounds = button.compute_bounds(&root);
        // A card can have valid coordinates outside one or more enclosing scroll viewports.
        // Only its visible intersection is a usable anchor for GTK's native popup layout.
        let mut visible = Some(graphene::Rect::new(
            0.0,
            0.0,
            root.width() as f32,
            root.height() as f32,
        ));
        let mut ancestor = self.parent();
        while let Some(widget) = ancestor {
            if widget.is::<gtk::ScrolledWindow>() {
                visible = visible.and_then(|clip| {
                    widget
                        .compute_bounds(&root)
                        .and_then(|bounds| clip.intersection(&bounds))
                });
            }
            ancestor = widget.parent();
        }
        let visible_anchor = bounds.and_then(|bounds| {
            visible
                .and_then(|clip| clip.intersection(&bounds))
                .filter(|bounds| bounds.width() > 0.0 && bounds.height() > 0.0)
        });
        let available = visible_anchor.map_or(0, |bounds| {
            (bounds.y() as i32).max(root.height() - (bounds.y() + bounds.height()).ceil() as i32)
        });
        let imp = self.imp();
        let centered = visible_anchor.is_none()
            || imp.details_force_centered.get()
            || details_should_center(
                available,
                imp.details_minimum_height.get(),
                imp.details_centered.get(),
            );
        imp.details_centered.set(centered);
        if !centered {
            let Some(bounds) = visible_anchor else { return };
            let Some(point) =
                root.compute_point(self, &graphene::Point::new(bounds.x(), bounds.y()))
            else {
                return;
            };
            let pointing_to = gtk::Allocation::new(
                point.x().round() as i32,
                point.y().round() as i32,
                bounds.width().ceil() as i32,
                bounds.height().ceil() as i32,
            );
            if popover.pointing_to() != (true, pointing_to) {
                popover.set_pointing_to(Some(&pointing_to));
            }
            popover.set_has_arrow(true);
            popover.set_offset(0, 0);
            popover.set_position(gtk::PositionType::Bottom);
        } else {
            // Vertical popovers can only slide horizontally. GTK dismisses them if the
            // compositor shrinks them below their minimum height. A side placement can slide
            // vertically across the entire monitor. Center it on the window rather than the
            // event, and keep it inside the window horizontally. The panel may overlap the
            // event, so omit the arrow rather than pointing at an unrelated location.
            popover.set_has_arrow(false);
            popover.set_position(gtk::PositionType::Right);
            let width = imp.details_width.get();
            let center_x = bounds.map_or(root.width() / 2, |bounds| {
                (bounds.x() + bounds.width() / 2.0).round() as i32
            });
            let x = if width > root.width() {
                (root.width() - width) / 2
            } else {
                (center_x - width / 2).clamp(0, root.width() - width)
            };
            // Use an in-window point, even when the event has scrolled off-screen. GDK
            // clips anchor rectangles to the native surface before applying offsets.
            let Some(point) = root.compute_point(
                self,
                &graphene::Point::new((x + width / 2) as f32, (root.height() / 2) as f32),
            ) else {
                return;
            };
            let pointing_to =
                gtk::Allocation::new(point.x().round() as i32, point.y().round() as i32, 1, 1);
            if popover.pointing_to() != (true, pointing_to) {
                popover.set_pointing_to(Some(&pointing_to));
            }
            popover.set_offset(-width / 2 - 1, 0);
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
        // Measure once in the default above/below mode. Its arrow and width requests change
        // in centered mode, so remeasuring it there would feed placement back into sizing.
        // Allow for wrapping even if the compositor reduces the popup to its minimum width.
        let (width, minimum_height) = popover.placement_size();
        self.imp().details_width.set(width);
        self.imp().details_minimum_height.set(minimum_height);
        self.imp().details_centered.set(false);
        self.imp().details_force_centered.set(false);
        self.imp().anchor.replace(Some(button.downgrade()));
        self.place_details(&popover);
        let weak = self.downgrade();
        popover.connect_closed(move |closed| {
            let weak = weak.clone();
            let closed = closed.clone();
            glib::idle_add_local_once(move || {
                if let Some(timeline) = weak.upgrade()
                    && timeline.imp().popover.borrow().as_ref() == Some(&closed)
                {
                    if closed.is_visible() {
                        return;
                    }
                    timeline.imp().popover.take();
                    timeline.imp().anchor.take();
                }
                if closed.parent().is_some() {
                    closed.unparent();
                }
            });
        });
        self.imp().popover.replace(Some(popover.clone()));
        popover.popup();
        self.check_details_mapping(&popover);
    }

    fn retry_details_centered(&self, popover: &EventDetails) -> bool {
        if self.imp().details_force_centered.get()
            || !self.is_mapped()
            || self.imp().popover.borrow().as_ref() != Some(popover)
        {
            return false;
        }
        // Window-relative room is only an estimate: the compositor also constrains popups
        // to the monitor. Once it rejects an anchored placement, stay centered for this popup.
        self.imp().details_force_centered.set(true);
        self.imp().details_centered.set(true);
        self.place_details(popover);
        popover.popup();
        self.check_details_mapping(popover);
        true
    }

    fn check_details_mapping(&self, popover: &EventDetails) {
        let weak = self.downgrade();
        let popover = popover.clone();
        glib::idle_add_local_once(move || {
            let Some(timeline) = weak.upgrade() else {
                return;
            };
            if timeline.imp().popover.borrow().as_ref() == Some(&popover)
                && !popover.is_mapped()
                && popover.is_visible()
                && !timeline.retry_details_centered(&popover)
            {
                timeline.close_details();
            }
        });
    }
}

/// Keeps centered placement until there is clearly enough room beside the event again.
fn details_should_center(available: i32, minimum: i32, centered: bool) -> bool {
    let threshold = minimum
        + DETAILS_PLACEMENT_MARGIN
        + if centered {
            DETAILS_PLACEMENT_HYSTERESIS
        } else {
            0
        };
    available < threshold
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

#[cfg(test)]
mod tests {
    use super::details_should_center;

    #[test]
    fn placement_requires_extra_room_before_leaving_centered_mode() {
        let mut centered = false;
        for available in [515, 520, 514, 530, 516, 560, 579] {
            centered = details_should_center(available, 500, centered);
            assert!(centered, "placement changed inside the hysteresis band");
        }
        centered = details_should_center(580, 500, centered);
        assert!(!centered);
        for available in [579, 530, 516] {
            centered = details_should_center(available, 500, centered);
            assert!(!centered, "anchored placement changed while it still fits");
        }
        assert!(details_should_center(515, 500, centered));
    }
}
