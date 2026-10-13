// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Owned, toolkit-independent data for calendar views.

pub mod details;
pub mod layout;
pub mod weekly;

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
