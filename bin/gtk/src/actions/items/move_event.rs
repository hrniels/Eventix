// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{DateTime, NaiveDateTime, Utc};
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
    let weak = grid.downgrade();
    target.connect_drop(move |_, value, x, y| {
        let Some(grid) = weak.upgrade() else {
            return false;
        };
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
