// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime};
use chrono_tz::Tz;
use eventix_ical::objects::{CalDate, CalDateTime, CalendarTimeZoneResolver, RangeEdge};
use eventix_state::actions::{
    Action,
    items::{ItemId, ResizeEvent},
};
use eventix_view::TimedEvent;
use gtk::{gdk, graphene, prelude::*};

use crate::view::{card::EventCard, page::CalendarPage, weekly::timed_grid::TimedGrid};

type TimeRange = (DateTime<Tz>, DateTime<Tz>);

struct Resize {
    segment: TimedEvent,
    edge: RangeEdge,
    pointer_y: f64,
    grid: TimedGrid,
    value: Option<NaiveDateTime>,
}

impl Resize {
    fn begin(card: &EventCard, x: f64, y: f64) -> Option<Self> {
        let edge = card.resize_edge(y)?;
        let page = card
            .ancestor(CalendarPage::static_type())
            .and_downcast::<CalendarPage>()?;
        if !page.can_edit() {
            return None;
        }
        let grid = card.parent()?.downcast::<TimedGrid>().ok()?;
        let point = card.compute_point(&grid, &graphene::Point::new(x as f32, y as f32))?;
        page.dismiss_popovers();
        Some(Self {
            segment: card.resize_segment()?,
            edge,
            pointer_y: f64::from(point.y()),
            grid,
            value: None,
        })
    }

    fn update(&mut self, offset: f64) {
        if offset.abs() < 8.0 && self.value.is_none() {
            return;
        }
        let Some((value, range)) = resize_range(
            self.segment.date,
            (self.segment.event.start, self.segment.event.end),
            self.edge,
            self.pointer_y + offset,
        ) else {
            self.value = None;
            self.grid.clear_drop_preview();
            return;
        };
        self.value = Some(value);
        self.grid.set_resize_preview(range);
    }

    fn command(&self) -> Option<Box<dyn Action>> {
        let event = &self.segment.event;
        let value = self.value?;
        let original = match self.edge {
            RangeEdge::Start => event.start.naive_local(),
            RangeEdge::End => event.end.naive_local(),
        };
        if value == original {
            return None;
        }
        Some(Box::new(ResizeEvent {
            item: ItemId {
                calendar: event.id.calendar.clone(),
                uid: event.id.uid.clone(),
                recurrence: if event.id.recurrence.is_empty() {
                    None
                } else {
                    Some(event.id.recurrence.parse().ok()?)
                },
            },
            edge: self.edge,
            value,
            expected_start: event.start.to_utc(),
            expected_end: event.end.to_utc(),
        }))
    }
}

/// Installs edge gestures ahead of the button and move-drag controllers.
pub(crate) fn install(card: &EventCard) {
    install_cursor(card);
    let active = Rc::new(RefCell::new(None::<Resize>));
    let drag = gtk::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = card.downgrade();
    let state = active.clone();
    drag.connect_drag_begin(move |gesture, x, y| {
        let Some(card) = weak.upgrade() else { return };
        if let Some(resize) = Resize::begin(&card, x, y) {
            card.grab_focus();
            card.set_cursor_from_name(Some(match resize.edge {
                RangeEdge::Start => "n-resize",
                RangeEdge::End => "s-resize",
            }));
            state.replace(Some(resize));
            gesture.set_state(gtk::EventSequenceState::Claimed);
        } else {
            gesture.set_state(gtk::EventSequenceState::Denied);
        }
    });
    let state = active.clone();
    drag.connect_drag_update(move |_, _, offset| {
        if let Some(resize) = state.borrow_mut().as_mut() {
            resize.update(offset);
        }
    });
    let state = active.clone();
    let weak = card.downgrade();
    drag.connect_drag_end(move |_, _, offset| {
        if let Some(mut resize) = state.take() {
            resize.update(offset);
            resize.grid.clear_drop_preview();
            if let Some(command) = resize.command()
                && let Some(page) = resize
                    .grid
                    .ancestor(CalendarPage::static_type())
                    .and_downcast::<CalendarPage>()
            {
                page.execute_command(command);
            }
        }
        if let Some(card) = weak.upgrade() {
            card.set_cursor_from_name(Some("pointer"));
        }
    });
    let state = active.clone();
    let weak = card.downgrade();
    drag.connect_cancel(move |_, _| {
        if let Some(resize) = state.take() {
            resize.grid.clear_drop_preview();
        }
        if let Some(card) = weak.upgrade() {
            card.set_cursor_from_name(Some("pointer"));
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = drag.downgrade();
    let state = active.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape && state.borrow().is_some() {
            if let Some(drag) = weak.upgrade() {
                drag.reset();
            }
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    card.add_controller(keys);
    card.add_controller(drag);

    card.connect_unmap(move |card| {
        if let Some(resize) = active.take() {
            resize.grid.clear_drop_preview();
        }
        card.set_cursor_from_name(Some("pointer"));
    });
}

fn install_cursor(card: &EventCard) {
    card.set_cursor_from_name(Some("pointer"));
    let motion = gtk::EventControllerMotion::new();
    let weak = card.downgrade();
    motion.connect_motion(move |_, _, y| {
        if let Some(card) = weak.upgrade() {
            let editable = card
                .ancestor(CalendarPage::static_type())
                .and_downcast::<CalendarPage>()
                .is_some_and(|page| page.can_edit());
            let edge = editable.then(|| card.resize_edge(y)).flatten();
            card.set_cursor_from_name(Some(match edge {
                Some(RangeEdge::Start) => "n-resize",
                Some(RangeEdge::End) => "s-resize",
                None => "pointer",
            }));
        }
    });
    let weak = card.downgrade();
    motion.connect_leave(move |_| {
        if let Some(card) = weak.upgrade() {
            card.set_cursor_from_name(Some("pointer"));
        }
    });
    card.add_controller(motion);
}

/// Snaps the selected edge within its day and preserves at least fifteen elapsed minutes.
fn resize_range(
    day: NaiveDate,
    (start, end): TimeRange,
    edge: RangeEdge,
    y: f64,
) -> Option<(NaiveDateTime, TimeRange)> {
    if !y.is_finite() {
        return None;
    }
    let tz = start.timezone();
    let midnight = day.and_hms_opt(0, 0, 0)?;
    let mut minute = (y.clamp(0.0, 1440.0) / 15.0).round() as i64 * 15;
    let step = match edge {
        RangeEdge::Start => -15,
        RangeEdge::End => 15,
    };
    // Search towards the fixed edge's opposite direction when the pointer crosses it.
    while (0..=1440).contains(&minute) {
        let value = midnight.checked_add_signed(Duration::minutes(minute))?;
        let date = CalDate::DateTime(CalDateTime::Timezone(value, tz.name().into()));
        let resolver = CalendarTimeZoneResolver::default();
        let range = match edge {
            RangeEdge::Start => (
                date.as_start_with_resolver(&tz, &resolver)
                    .with_timezone(&tz),
                end,
            ),
            RangeEdge::End => (
                start,
                date.as_end_with_resolver(&tz, &resolver).with_timezone(&tz),
            ),
        };
        if range.1 - range.0 >= Duration::minutes(15) {
            return Some((value, range));
        }
        minute += step;
    }
    None
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use chrono_tz::Europe::Berlin;
    use eventix_ical::objects::RangeEdge;

    use super::resize_range;

    #[test]
    fn edges_snap_to_quarter_hours_and_keep_a_minimum_duration() {
        let start = Berlin.with_ymd_and_hms(2026, 10, 14, 9, 0, 0).unwrap();
        let end = start + Duration::hours(1);
        for (edge, y, expected_start, expected_end) in [
            (RangeEdge::Start, 553.0, "09:15:00", "10:00:00"),
            (RangeEdge::End, 638.0, "09:00:00", "10:45:00"),
            (RangeEdge::Start, 700.0, "09:45:00", "10:00:00"),
            (RangeEdge::End, 400.0, "09:00:00", "09:15:00"),
        ] {
            let (_, (actual_start, actual_end)) =
                resize_range(start.date_naive(), (start, end), edge, y).unwrap();
            assert_eq!(actual_start.time().to_string(), expected_start);
            assert_eq!(actual_end.time().to_string(), expected_end);
        }
        assert!(resize_range(start.date_naive(), (start, end), RangeEdge::End, f64::NAN).is_none());
    }

    #[test]
    fn multiday_resize_keeps_the_other_edge_and_supports_midnight() {
        let start = Berlin.with_ymd_and_hms(2026, 10, 14, 23, 0, 0).unwrap();
        let end = start + Duration::hours(3);
        let (_, range) =
            resize_range(end.date_naive(), (start, end), RangeEdge::End, 1445.0).unwrap();
        assert_eq!(range.0, start);
        assert_eq!(range.1.naive_local().to_string(), "2026-10-16 00:00:00");
        let (_, range) =
            resize_range(start.date_naive(), (start, end), RangeEdge::Start, -10.0).unwrap();
        assert_eq!(range.0.naive_local().to_string(), "2026-10-14 00:00:00");
        assert_eq!(range.1, end);
    }

    #[test]
    fn resizing_resolves_dst_gaps_and_folds_using_the_selected_edge() {
        for (month, day, expected_end) in [
            (3, 29, "2026-03-29T03:15:00+02:00"),
            (10, 25, "2026-10-25T02:15:00+02:00"),
        ] {
            let start = Berlin.with_ymd_and_hms(2026, month, day, 1, 0, 0).unwrap();
            let end = start + Duration::hours(3);
            let (_, range) =
                resize_range(start.date_naive(), (start, end), RangeEdge::End, 135.0).unwrap();
            assert_eq!(range.0, start);
            assert_eq!(range.1.to_rfc3339(), expected_end);
        }
    }
}
