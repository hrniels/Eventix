// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime};
use chrono_tz::Tz;
use eventix_ical::objects::{CalDate, CalDateTime, CalendarTimeZoneResolver};
use gtk::{gdk, prelude::*};

use super::{card::EventCard, creation_hint::is_creation_target, page::CalendarPage};

const CONTROLLER_NAME: &str = "eventix-create-selection";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SelectionPoint {
    Timed(NaiveDateTime),
    AllDay(NaiveDate),
}

/// A creation range, with an inclusive last date for all-day selections.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SelectionRange {
    Timed(NaiveDateTime, NaiveDateTime),
    AllDay(NaiveDate, NaiveDate),
}

impl SelectionRange {
    pub(crate) fn between(anchor: SelectionPoint, pointer: SelectionPoint) -> Option<Self> {
        match (anchor, pointer) {
            (SelectionPoint::Timed(a), SelectionPoint::Timed(b)) => {
                let start = a.min(b);
                let end = a
                    .max(b)
                    .max(start.checked_add_signed(Duration::minutes(15))?);
                Some(Self::Timed(start, end))
            }
            (SelectionPoint::AllDay(a), SelectionPoint::AllDay(b)) => {
                Some(Self::AllDay(a.min(b), a.max(b)))
            }
            _ => None,
        }
    }

    /// Resolves local bounds using the calendar's start/end gap and fold policies.
    pub(crate) fn timed_bounds(self, tz: &Tz) -> Option<(DateTime<Tz>, DateTime<Tz>)> {
        let Self::Timed(start, end) = self else {
            return None;
        };
        let resolver = CalendarTimeZoneResolver::default();
        let start = CalDate::DateTime(CalDateTime::Timezone(start, tz.name().into()))
            .as_start_with_resolver(tz, &resolver)
            .with_timezone(tz);
        let end = CalDate::DateTime(CalDateTime::Timezone(end, tz.name().into()))
            .as_end_with_resolver(tz, &resolver)
            .with_timezone(tz);
        // A gap can resolve the anchor past the pointer; keep the result editable.
        let end = end.max(start.checked_add_signed(Duration::minutes(15))?);
        Some((start, end))
    }
}

struct Selection {
    anchor: SelectionPoint,
    origin: (f64, f64),
    active: bool,
}

/// Installs background selection without claiming ordinary clicks or card gestures.
pub(crate) fn install<W: IsA<gtk::Widget> + Clone + 'static>(
    widget: &W,
    point_at: impl Fn(&W, f64, f64, bool) -> Option<SelectionPoint> + 'static,
    preview: impl Fn(&W, SelectionRange) -> bool + 'static,
    clear: impl Fn(&W) + 'static,
) {
    widget.set_focusable(true);
    let point_at = Rc::new(point_at);
    let preview = Rc::new(preview);
    let clear = Rc::new(clear);
    let state = Rc::new(RefCell::new(None::<Selection>));
    let drag = gtk::GestureDrag::new();
    drag.set_name(Some(CONTROLLER_NAME));
    drag.set_button(gdk::BUTTON_PRIMARY);
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);

    let active = state.clone();
    let point = point_at.clone();
    let weak = widget.downgrade();
    drag.connect_drag_begin(move |gesture, x, y| {
        let Some(widget) = weak.upgrade() else { return };
        let page = widget
            .ancestor(CalendarPage::static_type())
            .and_downcast::<CalendarPage>();
        if !page.is_some_and(|page| page.can_create_event())
            || !is_background(widget.as_ref(), x, y)
        {
            gesture.set_state(gtk::EventSequenceState::Denied);
            return;
        }
        let Some(anchor) = point(&widget, x, y, false) else {
            gesture.set_state(gtk::EventSequenceState::Denied);
            return;
        };
        active.replace(Some(Selection {
            anchor,
            origin: (x, y),
            active: false,
        }));
    });

    let active = state.clone();
    let weak = widget.downgrade();
    let show = preview.clone();
    let point = point_at.clone();
    let cleanup = clear.clone();
    drag.connect_drag_update(move |gesture, dx, dy| {
        let Some(widget) = weak.upgrade() else { return };
        let mut state = active.borrow_mut();
        let Some(selection) = state.as_mut() else {
            return;
        };
        if !selection.active {
            if !widget.drag_check_threshold(
                selection.origin.0 as i32,
                selection.origin.1 as i32,
                (selection.origin.0 + dx) as i32,
                (selection.origin.1 + dy) as i32,
            ) {
                return;
            }
            selection.active = true;
            gesture.set_state(gtk::EventSequenceState::Claimed);
            widget.grab_focus();
            if let Some(page) = widget
                .ancestor(CalendarPage::static_type())
                .and_downcast::<CalendarPage>()
            {
                page.dismiss_popovers();
            }
        }
        let range = point(
            &widget,
            selection.origin.0 + dx,
            selection.origin.1 + dy,
            true,
        )
        .and_then(|pointer| SelectionRange::between(selection.anchor, pointer))
        .filter(|range| show(&widget, *range));
        if range.is_none() {
            cleanup(&widget);
        }
    });

    let active = state.clone();
    let weak = widget.downgrade();
    let cleanup = clear.clone();
    drag.connect_drag_end(move |_, dx, dy| {
        let selection = active.take();
        let Some(widget) = weak.upgrade() else { return };
        cleanup(&widget);
        if let Some(selection) = selection.filter(|selection| selection.active)
            && let Some(range) = point_at(
                &widget,
                selection.origin.0 + dx,
                selection.origin.1 + dy,
                true,
            )
            .and_then(|pointer| SelectionRange::between(selection.anchor, pointer))
            && let Some(page) = widget
                .ancestor(CalendarPage::static_type())
                .and_downcast::<CalendarPage>()
            && page.can_create_event()
        {
            page.create_event_range(range);
        }
    });

    let active = state.clone();
    let weak = widget.downgrade();
    drag.connect_cancel(move |_, _| {
        active.take();
        if let Some(widget) = weak.upgrade() {
            clear(&widget);
        }
    });

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = drag.downgrade();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape && state.borrow().is_some() {
            if let Some(drag) = weak.upgrade() {
                drag.reset();
            }
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    widget.add_controller(keys);
    widget.add_controller(drag);
    widget.connect_unmap(cancel);
}

pub(crate) fn cancel(widget: &impl IsA<gtk::Widget>) {
    for controller in widget.observe_controllers().snapshot() {
        if let Ok(drag) = controller.downcast::<gtk::GestureDrag>()
            && drag.name().as_deref() == Some(CONTROLLER_NAME)
        {
            drag.reset();
        }
    }
}

fn is_background(widget: &gtk::Widget, x: f64, y: f64) -> bool {
    if !is_creation_target(widget, x, y) {
        return false;
    }
    let mut picked = widget.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(child) = picked {
        if child.is::<EventCard>() {
            return false;
        }
        if child == *widget {
            return true;
        }
        picked = child.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use chrono_tz::Europe::Berlin;

    use super::{SelectionPoint, SelectionRange};

    #[test]
    #[ignore = "requires a GTK display"]
    fn creation_starts_only_on_background_not_cards_or_controls() {
        use gtk::prelude::*;

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let area = gtk::Fixed::new();
        let card = crate::view::card::EventCard::new();
        let button = gtk::Button::with_label("Control");
        let scrollbar = gtk::Scrollbar::new(gtk::Orientation::Horizontal, None::<&gtk::Adjustment>);
        for (widget, y) in [
            (card.upcast_ref::<gtk::Widget>(), 0.0),
            (button.upcast_ref(), 60.0),
            (scrollbar.upcast_ref(), 120.0),
        ] {
            widget.set_size_request(120, 40);
            area.put(widget, 0.0, y);
        }
        let window = gtk::Window::builder()
            .child(&area)
            .default_width(300)
            .default_height(220)
            .build();
        window.present();
        for _ in 0..20 {
            while gtk::glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        for y in [20.0, 80.0, 140.0] {
            assert!(!super::is_background(area.upcast_ref(), 20.0, y));
        }
        assert!(super::is_background(area.upcast_ref(), 200.0, 180.0));
        window.close();
    }

    #[test]
    fn timed_selection_orders_endpoints_and_keeps_a_quarter_hour_minimum() {
        let start = "2026-10-19T09:30:00".parse().unwrap();
        for end in [
            "2026-10-19T11:00:00",
            "2026-10-21T11:00:00",
            "2026-10-20T00:00:00",
        ] {
            let end = end.parse().unwrap();
            for (a, b) in [(start, end), (end, start)] {
                assert_eq!(
                    SelectionRange::between(SelectionPoint::Timed(a), SelectionPoint::Timed(b)),
                    Some(SelectionRange::Timed(start, end)),
                );
            }
        }
        assert_eq!(
            SelectionRange::between(SelectionPoint::Timed(start), SelectionPoint::Timed(start)),
            Some(SelectionRange::Timed(start, start + Duration::minutes(15))),
        );
    }

    #[test]
    fn all_day_selection_is_inclusive_and_independent_of_drag_direction() {
        let start = "2026-10-25".parse().unwrap();
        let end = "2026-11-03".parse().unwrap();
        for (a, b) in [(start, end), (end, start), (start, start)] {
            assert_eq!(
                SelectionRange::between(SelectionPoint::AllDay(a), SelectionPoint::AllDay(b)),
                Some(SelectionRange::AllDay(a.min(b), a.max(b))),
            );
        }
    }

    #[test]
    fn timed_selection_resolves_dst_bounds_and_preserves_a_minimum_elapsed_duration() {
        for (start, end, expected_start, expected_end) in [
            (
                "2026-03-29T02:15:00",
                "2026-03-29T03:45:00",
                "2026-03-29T03:15:00+02:00",
                "2026-03-29T03:45:00+02:00",
            ),
            (
                "2026-10-25T02:15:00",
                "2026-10-25T02:45:00",
                "2026-10-25T02:15:00+02:00",
                "2026-10-25T02:45:00+02:00",
            ),
            (
                "2026-03-29T02:45:00",
                "2026-03-29T03:00:00",
                "2026-03-29T03:45:00+02:00",
                "2026-03-29T04:00:00+02:00",
            ),
        ] {
            let (start, end) = SelectionRange::Timed(start.parse().unwrap(), end.parse().unwrap())
                .timed_bounds(&Berlin)
                .unwrap();
            assert_eq!(start.to_rfc3339(), expected_start);
            assert_eq!(end.to_rfc3339(), expected_end);
        }
    }
}
