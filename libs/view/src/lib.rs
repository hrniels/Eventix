// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Owned, toolkit-independent data for calendar views.

pub mod details;
pub mod editor;
pub mod layout;
pub mod monthly;
mod projection;
pub mod sidebar;
pub mod weekly;

pub use monthly::{MonthDay, MonthEvent, MonthSnapshot, month_range, shift_month};
pub use weekly::{AllDaySpan, Day, DstRange, TimedEvent, TimedSegmentId, WeekSnapshot, week_start};

use std::sync::Arc;

use chrono::DateTime;
use chrono_tz::Tz;

/// Stable identity of an occurrence, independent of its displayed placement.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OccurrenceId {
    pub calendar: String,
    pub uid: String,
    pub recurrence: String,
}

/// Owned occurrence data, including its original range for the details view.
#[derive(Clone, Debug)]
pub struct Event {
    pub id: OccurrenceId,
    pub summary: String,
    pub calendar: String,
    pub foreground: String,
    pub background: String,
    pub details: Arc<details::Details>,
    pub start: DateTime<Tz>,
    pub end: DateTime<Tz>,
    pub all_day: bool,
    pub cancelled: bool,
    pub recurrent: bool,
    pub exception: bool,
    pub alarm: bool,
}

impl Event {
    /// Loads the current presentation of an event by its stable occurrence identity.
    pub fn load(
        state: &eventix_state::State,
        item: &eventix_state::actions::items::ItemId,
    ) -> Option<Self> {
        use eventix_ical::objects::EventLike;
        let file = state.store().file_by_id(&item.uid)?;
        let occ = file.occurrence_by_id(&item.uid, item.recurrence.as_ref(), state.timezone())?;
        let (_, settings) = state.settings().calendar(&item.calendar)?;
        let start = occ.occurrence_start()?.with_timezone(state.timezone());
        Some(Self {
            id: OccurrenceId {
                calendar: item.calendar.clone(),
                uid: item.uid.clone(),
                recurrence: item
                    .recurrence
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            },
            summary: occ.summary().cloned().unwrap_or_default(),
            calendar: settings.name().clone(),
            foreground: settings.fgcolor().clone(),
            background: settings.bgcolor().clone(),
            details: Arc::new(details::Details::load(state, &occ)?),
            start,
            end: occ
                .occurrence_end()
                .unwrap_or(start)
                .with_timezone(state.timezone()),
            all_day: occ.is_all_day(),
            cancelled: occ.is_cancelled(),
            recurrent: occ.is_recurrent(),
            exception: occ.is_overwritten(),
            alarm: state.personal_alarms().has_alarms(&occ, settings.alarms()),
        })
    }
}
