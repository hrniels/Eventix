// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use eventix_ical::objects::{CalTodoStatus, EventLike};
use tempfile::TempDir;

use crate::helper::edit::read_ics_by_uid;
use crate::helper::{CAL_ID, encode_form, make_router, make_state, post_query};

use super::write_recurring_event_ics;

// --- POST /api/items/complete ---

/// Writes a minimal VTODO ICS file for `uid` into `cal_dir`.
///
/// The todo has no start or due date, just the given summary.
fn write_todo_ics(cal_dir: &Path, uid: &str, summary: &str) {
    std::fs::write(
        cal_dir.join(format!("{uid}.ics")),
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VTODO\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             SUMMARY:{summary}\r\n\
             END:VTODO\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
}

/// Completing a simple VTODO sets STATUS:COMPLETED and PERCENT-COMPLETE:100.
#[tokio::test]
async fn complete_todo_basic() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "complete-basic";
    write_todo_ics(&cal_dir, uid, "Do laundry");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid)]);
    let (status, _) = post_query(router, &format!("/api/items/complete?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    assert_eq!(
        comp.as_todo().unwrap().status(),
        Some(CalTodoStatus::Completed)
    );
    assert_eq!(comp.as_todo().unwrap().percent(), Some(100));
    assert!(comp.as_todo().unwrap().completed().is_some());
}

/// Completing a specific occurrence of a recurring VTODO creates a RECURRENCE-ID override with
/// STATUS:COMPLETED.
#[tokio::test]
async fn complete_recurring_occurrence_creates_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "complete-recurring";

    // Write a recurring VTODO (weekly, starting 2026-04-15).
    let path = cal_dir.join(format!("{uid}.ics"));
    std::fs::write(
        &path,
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VTODO\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             DTSTART;TZID=Europe/Berlin:20260415T090000\r\n\
             DUE;TZID=Europe/Berlin:20260415T100000\r\n\
             RRULE:FREQ=WEEKLY;BYDAY=WE\r\n\
             SUMMARY:Weekly review\r\n\
             END:VTODO\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Complete the first occurrence.
    let qs = encode_form(&[("uid", uid), ("rid", "TTEurope/Berlin;2026-04-15T09:00:00")]);
    let (status, _) = post_query(router, &format!("/api/items/complete?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comps = ics.components();
    let override_comp = comps
        .iter()
        .find(|c| c.rid().is_some())
        .expect("expected a RECURRENCE-ID override");
    assert_eq!(
        override_comp.as_todo().unwrap().status(),
        Some(CalTodoStatus::Completed)
    );
}

/// Completing a non-recurrent component when a `rid` is supplied but no matching override exists
/// returns an error.
#[tokio::test]
async fn non_recurrent_with_rid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "complete-non-recurrent";
    write_todo_ics(&cal_dir, uid, "Single task");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Supplying a rid for a non-recurrent component should fail.
    let qs = encode_form(&[("uid", uid), ("rid", "TTEurope/Berlin;2026-04-15T09:00:00")]);
    let (status, _) = post_query(router, &format!("/api/items/complete?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}

/// Supplying an unknown UID returns an error.
#[tokio::test]
async fn unknown_uid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_recurring_event_ics(&cal_dir, "something-else");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", "no-such-uid")]);
    let (status, _) = post_query(router, &format!("/api/items/complete?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}
