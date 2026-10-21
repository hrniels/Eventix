// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::Cell, rc::Rc};

use chrono::{DateTime, NaiveDateTime, Utc};
use chrono_tz::Tz;
use eventix_ical::objects::{CalDate, CalDateTime, CalendarTimeZoneResolver};
use eventix_state::actions::{
    Command,
    items::{ItemId, MoveEvent},
};
use eventix_view::TimedEvent;
use gtk::{gdk, glib, prelude::*};

use crate::view::{card::EventCard, page::CalendarPage, weekly::timed_grid::TimedGrid};

/// A process-local drag carries identity and timing, never a calendar object or a widget.
#[derive(Clone, Debug, glib::Boxed)]
#[boxed_type(name = "EventixDraggedEvent")]
pub(crate) struct DraggedEvent {
    item: ItemId,
    expected_start: DateTime<Utc>,
    expected_end: DateTime<Utc>,
    pub offset_minutes: i64,
}

impl DraggedEvent {
    pub fn new(segment: &TimedEvent) -> Option<Self> {
        let event = &segment.event;
        if !event.details.actions.edit {
            return None;
        }
        let recurrence = if event.id.recurrence.is_empty() {
            None
        } else {
            Some(event.id.recurrence.parse().ok()?)
        };
        let displayed_start = segment.date.and_hms_opt(0, 0, 0)?
            + chrono::Duration::minutes(i64::from(segment.minute_start.min(1422)));
        Some(Self {
            item: ItemId {
                calendar: event.id.calendar.clone(),
                uid: event.id.uid.clone(),
                recurrence,
            },
            expected_start: event.start.with_timezone(&Utc),
            expected_end: event.end.with_timezone(&Utc),
            offset_minutes: (displayed_start - event.start.naive_local()).num_minutes(),
        })
    }

    /// Returns the destination bounds with the same gap/fold and duration semantics as a move.
    pub fn preview_range(
        &self,
        start: NaiveDateTime,
        timezone: &Tz,
    ) -> Option<(DateTime<Tz>, DateTime<Tz>)> {
        let date = CalDate::DateTime(CalDateTime::Timezone(start, timezone.name().into()));
        let start = date
            .as_start_with_resolver(timezone, &CalendarTimeZoneResolver::default())
            .with_timezone(timezone);
        let end = start.checked_add_signed(self.expected_end - self.expected_start)?;
        Some((start, end))
    }

    fn command(&self, start: NaiveDateTime) -> Command {
        Command::MoveEvent(MoveEvent {
            item: self.item.clone(),
            start,
            expected_start: self.expected_start,
            expected_end: self.expected_end,
        })
    }
}

pub(crate) fn install_source(card: &EventCard) {
    let source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    let weak = card.downgrade();
    source.connect_prepare(move |_, _, y| {
        let card = weak.upgrade()?;
        let page = card
            .ancestor(CalendarPage::static_type())?
            .downcast::<CalendarPage>()
            .ok()?;
        if !page.can_edit() {
            return None;
        }
        let payload = card.drag_payload(y)?;
        page.dismiss_popovers();
        Some(gdk::ContentProvider::for_value(&payload.to_value()))
    });
    card.add_controller(source);
}

pub(crate) fn install_target(grid: &TimedGrid) {
    let target = gtk::DropTarget::new(DraggedEvent::static_type(), gdk::DragAction::MOVE);
    target.set_preload(true);
    let position = Rc::new(Cell::new(None));
    let last_position = position.clone();
    let weak = grid.downgrade();
    target.connect_enter(move |target, x, y| {
        last_position.set(Some((x, y)));
        weak.upgrade().map_or(gdk::DragAction::empty(), |grid| {
            preview(&grid, target, x, y)
        })
    });
    let last_position = position.clone();
    let weak = grid.downgrade();
    target.connect_motion(move |target, x, y| {
        last_position.set(Some((x, y)));
        weak.upgrade().map_or(gdk::DragAction::empty(), |grid| {
            preview(&grid, target, x, y)
        })
    });
    // Preloading can finish after enter, even when the pointer is already stationary.
    let last_position = position.clone();
    let weak = grid.downgrade();
    target.connect_value_notify(move |target| {
        if let Some(grid) = weak.upgrade()
            && let Some((x, y)) = last_position.get()
        {
            preview(&grid, target, x, y);
        }
    });
    let last_position = position.clone();
    let weak = grid.downgrade();
    target.connect_leave(move |_| {
        last_position.set(None);
        if let Some(grid) = weak.upgrade() {
            grid.clear_drop_preview();
        }
    });
    let weak = grid.downgrade();
    target.connect_drop(move |_, value, x, y| {
        position.set(None);
        let Some(grid) = weak.upgrade() else {
            return false;
        };
        grid.clear_drop_preview();
        let Ok(payload) = value.get::<DraggedEvent>() else {
            return false;
        };
        let Some(start) = grid.drop_start(x, y, payload.offset_minutes) else {
            return false;
        };
        let Some(page) = grid
            .ancestor(CalendarPage::static_type())
            .and_downcast::<CalendarPage>()
        else {
            return false;
        };
        page.execute_command(payload.command(start))
    });
    grid.add_controller(target);
}

fn preview(grid: &TimedGrid, target: &gtk::DropTarget, x: f64, y: f64) -> gdk::DragAction {
    if !grid
        .ancestor(CalendarPage::static_type())
        .and_downcast::<CalendarPage>()
        .is_some_and(|page| page.can_edit())
        || grid.drop_start(x, y, 0).is_none()
    {
        grid.clear_drop_preview();
        return gdk::DragAction::empty();
    }
    let payload = target
        .value()
        .and_then(|value| value.get::<DraggedEvent>().ok());
    let Some(payload) = payload else {
        grid.clear_drop_preview();
        return gdk::DragAction::MOVE;
    };
    if grid.set_drop_preview(&payload, x, y) {
        return gdk::DragAction::MOVE;
    }
    grid.clear_drop_preview();
    gdk::DragAction::empty()
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use chrono_tz::Europe::Berlin;
    use eventix_state::actions::items::ItemId;

    use super::DraggedEvent;

    #[test]
    fn preview_preserves_elapsed_duration_and_resolves_dst_gap_and_fold() {
        let expected_start = Utc.with_ymd_and_hms(2026, 10, 14, 8, 0, 0).unwrap();
        let payload = DraggedEvent {
            item: ItemId {
                calendar: String::new(),
                uid: String::new(),
                recurrence: None,
            },
            expected_start,
            expected_end: expected_start + Duration::hours(1),
            offset_minutes: 0,
        };
        for (destination, expected_start, expected_end) in [
            (
                "2026-10-14T10:00:00",
                "2026-10-14T10:00:00+02:00",
                "2026-10-14T11:00:00+02:00",
            ),
            (
                "2026-03-29T02:15:00",
                "2026-03-29T03:15:00+02:00",
                "2026-03-29T04:15:00+02:00",
            ),
            (
                "2026-10-25T02:15:00",
                "2026-10-25T02:15:00+02:00",
                "2026-10-25T02:15:00+01:00",
            ),
        ] {
            let (start, end) = payload
                .preview_range(destination.parse().unwrap(), &Berlin)
                .unwrap();
            assert_eq!(start.to_rfc3339(), expected_start);
            assert_eq!(end.to_rfc3339(), expected_end);
            assert_eq!(end - start, Duration::hours(1));
        }
    }
}
