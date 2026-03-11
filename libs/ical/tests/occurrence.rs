// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for [`Occurrence`] properties when obtained via [`CalFile`].
//!
//! These tests parse real `.ics` fixture files and exercise `Occurrence` getters that are
//! most naturally reached through a full parse-and-iterate round-trip.

use chrono::TimeZone;
use chrono_tz::UTC;

use eventix_ical::col::{CalFile, Occurrence};
use eventix_ical::objects::{CalEventStatus, CalTodoStatus};

mod common;
use common::{data_dir, make_id};

fn utc(year: i32, month: u32, day: u32, h: u32, m: u32, s: u32) -> chrono::DateTime<chrono_tz::Tz> {
    UTC.with_ymd_and_hms(year, month, day, h, m, s).unwrap()
}

fn first_occurrence(file: &CalFile) -> Occurrence<'_> {
    file.occurrences_between(
        utc(2025, 1, 1, 0, 0, 0),
        utc(2025, 12, 31, 23, 59, 59),
        |_| true,
    )
    .next()
    .expect("expected at least one occurrence")
}

/// Parses `todo_with_status.ics` and checks that all TODO-specific properties are accessible
/// through the `Occurrence` API and that `is_cancelled` returns `true`.
#[test]
fn todo_cancelled_and_properties_via_occurrence() {
    let path = data_dir().join("todo_with_status.ics");
    let file = CalFile::new_from_file(make_id("cal"), path).unwrap();

    let occ = first_occurrence(&file);

    assert_eq!(occ.todo_status(), Some(CalTodoStatus::Cancelled));
    assert_eq!(occ.todo_percent(), Some(50));
    assert!(occ.todo_completed().is_some(), "expected a COMPLETED date");
    assert!(occ.is_cancelled(), "expected occurrence to be cancelled");
}

/// Parses `event_cancelled.ics` and checks that `event_status` returns `Cancelled` and
/// `is_cancelled` returns `true` via the `Occurrence` API.
#[test]
fn event_cancelled_via_occurrence() {
    let path = data_dir().join("event_cancelled.ics");
    let file = CalFile::new_from_file(make_id("cal"), path).unwrap();

    let occ = first_occurrence(&file);

    assert_eq!(occ.event_status(), Some(CalEventStatus::Cancelled));
    assert!(occ.is_cancelled(), "expected occurrence to be cancelled");
}
