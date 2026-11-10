// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Owned presentation data, read-only constructors from state, and layout helpers.
//!
//! Constructors may read local calendar metadata and must run off the UI thread when doing so.
//! Mutations, persistence, and task coordination belong to the worker.

use eventix_ical::objects::CalPartStat;

pub(crate) mod activity;
pub(crate) mod calendars;
pub(crate) mod details;
pub(crate) mod editor;
pub(crate) mod import;
mod layout;
pub(crate) mod list;
pub(crate) mod list_occurrences;
pub(crate) mod monthly;
pub(crate) mod notifications;
pub(crate) mod occurrence;
pub(crate) mod sidebar;
pub(crate) mod weekly;

pub(crate) use monthly::{MonthDay, MonthEvent, MonthSnapshot};
pub(crate) use occurrence::{Event, OccurrenceId};
#[cfg(test)]
pub(crate) use weekly::Day;
pub(crate) use weekly::{AllDaySpan, DstRange, TimedEvent, TimedSegmentId, WeekSnapshot};

/// An available item action or invitation status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Complete,
    Postpone,
    Delete,
    Edit,
    Respond {
        status: CalPartStat,
        read_only: bool,
    },
    Alarms,
    CopyUid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum View {
    #[default]
    Week,
    Month,
    List,
}

pub(crate) enum Snapshot {
    Week(WeekSnapshot),
    Month(MonthSnapshot),
    List(list::Snapshot),
}

pub(crate) enum SavedItem {
    Event(Box<Event>),
    Task(Box<sidebar::SidebarRow>),
}
