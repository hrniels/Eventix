// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gtk::{gdk, graphene, prelude::*};

use crate::style;

use super::card::EventCard;
use super::geometry::rect;

const SCROLL_QUIET_PERIOD: Duration = Duration::from_millis(200);

/// Tracks a creation target without intercepting card clicks or drag gestures.
#[derive(Default)]
pub(crate) struct CreationHint {
    position: Cell<Option<(f64, f64)>>,
    scroll_until: Cell<Option<Instant>>,
}

impl CreationHint {
    pub(crate) fn position(&self) -> Option<(f64, f64)> {
        self.position.get()
    }

    pub(crate) fn clear(&self, widget: &impl IsA<gtk::Widget>) {
        if self.position.take().is_some() {
            widget.set_cursor_from_name(None);
            widget.queue_draw();
        }
    }

    fn pause_for_scroll(&self, widget: &impl IsA<gtk::Widget>) {
        self.scroll_until
            .set(Some(Instant::now() + SCROLL_QUIET_PERIOD));
        self.clear(widget);
    }

    pub(crate) fn install<W: IsA<gtk::Widget>>(
        self: &Rc<Self>,
        widget: &W,
        contains_slot: impl Fn(&W, f64, f64) -> bool + 'static,
    ) {
        let motion = gtk::EventControllerMotion::new();
        motion.set_propagation_phase(gtk::PropagationPhase::Capture);

        let hint = self.clone();
        motion.connect_motion(move |controller, x, y| {
            let Some(widget) = controller.widget().and_downcast::<W>() else {
                return;
            };

            // Touchpads can interleave pointer motion with scrolling. Wait for scrolling
            // to settle before allowing motion to restore the highlight and copy cursor.
            if hint
                .scroll_until
                .get()
                .is_some_and(|until| Instant::now() < until)
            {
                return;
            }

            let buttons = gdk::ModifierType::BUTTON1_MASK
                | gdk::ModifierType::BUTTON2_MASK
                | gdk::ModifierType::BUTTON3_MASK;
            if controller.current_event_state().intersects(buttons)
                || !is_creation_target(widget.as_ref(), x, y)
                || !contains_slot(&widget, x, y)
            {
                hint.clear(&widget);
            } else if hint.position.replace(Some((x, y))) != Some((x, y)) {
                widget.set_cursor_from_name(Some("copy"));
                widget.queue_draw();
            }
        });

        let hint = self.clone();
        motion.connect_leave(move |controller| {
            if let Some(widget) = controller.widget() {
                hint.clear(&widget);
            }
        });
        widget.add_controller(motion);

        // Clear stale hover feedback on touch or scrolling, but preserve it on mouse clicks.
        let events = gtk::EventControllerLegacy::new();
        events.set_propagation_phase(gtk::PropagationPhase::Capture);
        let hint = self.clone();
        events.connect_event(move |controller, event| {
            if let Some(widget) = controller.widget() {
                match event.event_type() {
                    gdk::EventType::Scroll => hint.pause_for_scroll(&widget),
                    gdk::EventType::TouchBegin => hint.clear(&widget),
                    _ => {}
                }
            }

            gtk::glib::Propagation::Proceed
        });
        widget.add_controller(events);

        let hint = self.clone();
        widget.connect_unmap(move |widget| hint.clear(widget));
    }
}

pub(crate) fn is_creation_target(widget: &gtk::Widget, x: f64, y: f64) -> bool {
    let mut picked = widget.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(child) = picked {
        if (child.is::<gtk::Button>() && !child.is::<EventCard>()) || child.is::<gtk::Scrollbar>() {
            return false;
        }

        if child == *widget {
            break;
        }

        picked = child.parent();
    }

    true
}

/// Shades a hovered slot with a subtle, theme-derived background.
pub(crate) fn snapshot_hint(
    widget: &impl IsA<gtk::Widget>,
    snapshot: &gtk::Snapshot,
    bounds: graphene::Rect,
) {
    rect(
        snapshot,
        &style::color(widget, "ev_create_background"),
        bounds.x() + 1.0,
        bounds.y() + 1.0,
        (bounds.width() - 2.0).max(0.0),
        (bounds.height() - 2.0).max(0.0),
    );
}

#[cfg(test)]
mod tests {
    use std::{rc::Rc, time::Instant};

    use gtk::prelude::*;

    use super::CreationHint;

    #[test]
    #[ignore = "requires a GTK display"]
    fn scrolling_suppresses_creation_feedback_until_motion_after_a_quiet_period() {
        gtk::init().unwrap();
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let hint = Rc::new(CreationHint::default());
        hint.install(&widget, |_, _, _| true);
        let motion = widget
            .observe_controllers()
            .snapshot()
            .into_iter()
            .find_map(|controller| controller.downcast::<gtk::EventControllerMotion>().ok())
            .unwrap();
        let move_pointer = |x: f64| motion.emit_by_name::<()>("motion", &[&x, &10.0_f64]);

        move_pointer(10.0);
        assert_eq!(hint.position(), Some((10.0, 10.0)));
        assert_eq!(widget.cursor().unwrap().name().as_deref(), Some("copy"));

        hint.pause_for_scroll(&widget);
        assert_eq!(hint.position(), None);
        assert!(widget.cursor().is_none());
        move_pointer(11.0);
        assert_eq!(hint.position(), None);
        assert!(widget.cursor().is_none());

        // A later scroll restarts suppression, including after pointer leave/re-entry.
        hint.scroll_until.set(Some(Instant::now()));
        hint.pause_for_scroll(&widget);
        motion.emit_by_name::<()>("leave", &[]);
        move_pointer(12.0);
        assert_eq!(hint.position(), None);
        assert!(widget.cursor().is_none());

        // Expiring the quiet period alone must not restore stale feedback.
        hint.scroll_until.set(Some(Instant::now()));
        assert_eq!(hint.position(), None);
        assert!(widget.cursor().is_none());
        move_pointer(13.0);
        assert_eq!(hint.position(), Some((13.0, 10.0)));
        assert_eq!(widget.cursor().unwrap().name().as_deref(), Some("copy"));
    }
}
