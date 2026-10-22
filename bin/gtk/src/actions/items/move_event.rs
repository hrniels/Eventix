// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::Cell, rc::Rc};

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::Tz;
use eventix_ical::objects::{CalDate, CalDateTime, CalendarTimeZoneResolver};
use eventix_state::actions::{
    Command,
    items::{CopyEvent, ItemId, MoveEvent},
};
use eventix_view::{Event, TimedEvent};
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
    pub all_day: bool,
    original_start: NaiveDateTime,
    original_end: NaiveDateTime,
}

// No drop target accepts this type, so GTK displays its disallowed-drag cursor.
#[derive(Clone, glib::Boxed)]
#[boxed_type(name = "EventixBlockedEvent")]
struct BlockedEvent;

impl DraggedEvent {
    pub fn new(segment: &TimedEvent) -> Option<Self> {
        let mut payload = Self::for_month(&segment.event)?;
        let displayed_start = segment.date.and_hms_opt(0, 0, 0)?
            + chrono::Duration::minutes(i64::from(segment.minute_start.min(1422)));
        payload.offset_minutes =
            (displayed_start - segment.event.start.naive_local()).num_minutes();
        Some(payload)
    }

    pub fn for_month(event: &Event) -> Option<Self> {
        if !event.details.actions.edit {
            return None;
        }
        let recurrence = if event.id.recurrence.is_empty() {
            None
        } else {
            Some(event.id.recurrence.parse().ok()?)
        };
        Some(Self {
            item: ItemId {
                calendar: event.id.calendar.clone(),
                uid: event.id.uid.clone(),
                recurrence,
            },
            expected_start: event.start.with_timezone(&Utc),
            expected_end: event.end.with_timezone(&Utc),
            offset_minutes: 0,
            all_day: event.all_day,
            original_start: event.start.naive_local(),
            original_end: event.end.naive_local(),
        })
    }

    /// Moves the occurrence's start date while retaining its exact local time of day.
    pub fn on_date(&self, date: NaiveDate) -> NaiveDateTime {
        date.and_time(self.original_start.time())
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
        let end = if self.all_day {
            let end = start
                .naive_local()
                .checked_add_signed(self.original_end - self.original_start)?;
            CalDate::DateTime(CalDateTime::Timezone(end, timezone.name().into()))
                .as_end_with_resolver(timezone, &CalendarTimeZoneResolver::default())
                .with_timezone(timezone)
        } else {
            start.checked_add_signed(self.expected_end - self.expected_start)?
        };
        Some((start, end))
    }

    fn command(&self, start: NaiveDateTime, copy: bool) -> Command {
        if copy {
            return Command::CopyEvent(CopyEvent {
                item: self.item.clone(),
                start,
                expected_start: self.expected_start,
                expected_end: self.expected_end,
            });
        }
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
        .actions(gdk::DragAction::MOVE | gdk::DragAction::COPY)
        .build();
    let weak = card.downgrade();
    source.connect_prepare(move |source, _, y| {
        let card = weak.upgrade()?;
        let page = card
            .ancestor(CalendarPage::static_type())?
            .downcast::<CalendarPage>()
            .ok()?;
        if page.can_edit() && card.resize_edge(y).is_some() {
            return None;
        }
        let payload = if page.can_edit() {
            card.drag_payload(y)
        } else {
            None
        };
        let content = match payload {
            Some(payload) => {
                source.set_actions(if payload.item.recurrence.is_none() {
                    gdk::DragAction::MOVE | gdk::DragAction::COPY
                } else {
                    gdk::DragAction::MOVE
                });
                payload.to_value()
            }
            None => BlockedEvent.to_value(),
        };
        source.set_icon(Some(&gdk::Paintable::new_empty(1, 1)), 0, 0);
        page.dismiss_popovers();
        Some(gdk::ContentProvider::for_value(&content))
    });
    card.add_controller(source);
}

pub(crate) trait MoveTarget: IsA<gtk::Widget> + Clone + 'static {
    fn move_start(&self, payload: &DraggedEvent, x: f64, y: f64) -> Option<NaiveDateTime>;
    fn show_move_preview(&self, payload: &DraggedEvent, x: f64, y: f64) -> bool;
    fn clear_drop_preview(&self);
}

impl MoveTarget for TimedGrid {
    fn move_start(&self, payload: &DraggedEvent, x: f64, y: f64) -> Option<NaiveDateTime> {
        if payload.all_day {
            return None;
        }
        self.drop_start(x, y, payload.offset_minutes)
    }

    fn show_move_preview(&self, payload: &DraggedEvent, x: f64, y: f64) -> bool {
        !payload.all_day && self.set_drop_preview(payload, x, y)
    }

    fn clear_drop_preview(&self) {
        self.clear_drop_preview();
    }
}

pub(crate) fn install_target<T: MoveTarget>(grid: &T) {
    let target = gtk::DropTarget::new(
        DraggedEvent::static_type(),
        gdk::DragAction::MOVE | gdk::DragAction::COPY,
    );
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
    target.connect_drop(move |target, value, x, y| {
        position.set(None);
        let Some(grid) = weak.upgrade() else {
            return false;
        };
        grid.clear_drop_preview();
        let Ok(payload) = value.get::<DraggedEvent>() else {
            return false;
        };
        let Some(start) = grid.move_start(&payload, x, y) else {
            return false;
        };
        let Some(page) = grid
            .ancestor(CalendarPage::static_type())
            .and_downcast::<CalendarPage>()
        else {
            return false;
        };
        let copy = target
            .current_drop()
            .and_then(|drop| drop.drag())
            .is_some_and(|drag| drag.selected_action() == gdk::DragAction::COPY);
        page.execute_command(payload.command(start, copy))
    });
    grid.add_controller(target);
}

fn preview<T: MoveTarget>(grid: &T, target: &gtk::DropTarget, x: f64, y: f64) -> gdk::DragAction {
    if !grid
        .ancestor(CalendarPage::static_type())
        .and_downcast::<CalendarPage>()
        .is_some_and(|page| page.can_edit())
    {
        grid.clear_drop_preview();
        return gdk::DragAction::empty();
    }
    let payload = target
        .value()
        .and_then(|value| value.get::<DraggedEvent>().ok());
    let Some(payload) = payload else {
        grid.clear_drop_preview();
        return drag_action(target);
    };
    if grid.show_move_preview(&payload, x, y) {
        return drag_action(target);
    }
    grid.clear_drop_preview();
    gdk::DragAction::empty()
}

fn drag_action(target: &gtk::DropTarget) -> gdk::DragAction {
    let allows_copy = target
        .current_drop()
        .is_some_and(|drop| drop.actions().contains(gdk::DragAction::COPY));
    if allows_copy
        && target
            .current_event_state()
            .contains(gdk::ModifierType::CONTROL_MASK)
    {
        gdk::DragAction::COPY
    } else {
        gdk::DragAction::MOVE
    }
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
            all_day: false,
            original_start: expected_start.naive_utc(),
            original_end: (expected_start + Duration::hours(1)).naive_utc(),
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

    #[test]
    fn monthly_moves_preserve_local_time_including_seconds_across_dst() {
        let start = Berlin.with_ymd_and_hms(2026, 10, 24, 23, 17, 42).unwrap();
        let payload = DraggedEvent {
            item: ItemId {
                calendar: String::new(),
                uid: String::new(),
                recurrence: None,
            },
            expected_start: start.to_utc(),
            expected_end: (start + Duration::hours(2)).to_utc(),
            offset_minutes: 1440,
            all_day: false,
            original_start: start.naive_local(),
            original_end: (start + Duration::hours(2)).naive_local(),
        };
        let destination = payload.on_date("2026-10-25".parse().unwrap());
        assert_eq!(destination.to_string(), "2026-10-25 23:17:42");
        let (start, end) = payload.preview_range(destination, &Berlin).unwrap();
        assert_eq!(start.to_rfc3339(), "2026-10-25T23:17:42+01:00");
        assert_eq!(end - start, Duration::hours(2));
    }

    #[test]
    fn all_day_preview_preserves_calendar_days_across_dst() {
        let start = Berlin.with_ymd_and_hms(2026, 10, 20, 0, 0, 0).unwrap();
        let end = start + Duration::days(2) - Duration::seconds(1);
        let payload = DraggedEvent {
            item: ItemId {
                calendar: String::new(),
                uid: String::new(),
                recurrence: None,
            },
            expected_start: start.to_utc(),
            expected_end: end.to_utc(),
            offset_minutes: 0,
            all_day: true,
            original_start: start.naive_local(),
            original_end: end.naive_local(),
        };
        let destination = payload.on_date("2026-10-24".parse().unwrap());
        let (start, end) = payload.preview_range(destination, &Berlin).unwrap();
        assert_eq!(start.to_rfc3339(), "2026-10-24T00:00:00+02:00");
        assert_eq!(end.to_rfc3339(), "2026-10-25T23:59:59+01:00");
    }
}
