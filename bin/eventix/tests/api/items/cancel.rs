// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use eventix_ical::objects::{CalEventStatus, EventLike};
use tempfile::TempDir;

use crate::helper::edit::read_ics_by_uid;
use crate::helper::{CAL_ID, encode_form, make_router, make_state, post_query};

use super::{write_event_ics, write_recurring_event_ics};

// --- POST /api/items/cancel ---

/// Writes a recurring event ICS that already has a RECURRENCE-ID override for 2026-04-15.
///
/// When `cancelled` is `true` the override carries `STATUS:CANCELLED` and the summary
/// `"Canceled: Weekly standup"`; otherwise it is a plain rescheduled occurrence with the summary
/// `"Rescheduled standup"`.
fn write_recurring_with_override(cal_dir: &Path, uid: &str, cancelled: bool) {
    let extra = if cancelled {
        "STATUS:CANCELLED\r\nSUMMARY:Canceled: Weekly standup\r\n"
    } else {
        "SUMMARY:Rescheduled standup\r\n"
    };
    let content = format!(
        "BEGIN:VCALENDAR\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         DTSTAMP:20260101T000000Z\r\n\
         DTSTART;TZID=Europe/Berlin:20260415T090000\r\n\
         DTEND;TZID=Europe/Berlin:20260415T100000\r\n\
         RRULE:FREQ=WEEKLY;BYDAY=WE\r\n\
         SUMMARY:Weekly standup\r\n\
         END:VEVENT\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         DTSTAMP:20260101T000000Z\r\n\
         RECURRENCE-ID;TZID=Europe/Berlin:20260415T090000\r\n\
         DTSTART;TZID=Europe/Berlin:20260415T090000\r\n\
         DTEND;TZID=Europe/Berlin:20260415T100000\r\n\
         {extra}\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n"
    );
    std::fs::write(cal_dir.join(format!("{uid}.ics")), content).unwrap();
}

/// Cancelling a specific occurrence of a recurring event creates a RECURRENCE-ID override with
/// STATUS:CANCELLED and the summary prefixed with "Canceled: ".
#[tokio::test]
async fn cancel_recurring_occurrence_creates_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "cancel-recurring";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // The recurring event starts 2026-04-15 09:00 Europe/Berlin; pass the rid in CalDate format.
    let qs = encode_form(&[("uid", uid), ("rid", "TTEurope/Berlin;2026-04-15T09:00:00")]);
    let (status, _) = post_query(router, &format!("/api/items/cancel?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    // The file now has a base component and an override.
    let comps = ics.components();
    let override_comp = comps
        .iter()
        .find(|c| c.rid().is_some())
        .expect("expected a RECURRENCE-ID override");
    assert_eq!(
        override_comp.as_event().unwrap().status(),
        Some(CalEventStatus::Cancelled)
    );
    assert_eq!(
        override_comp.summary(),
        Some(&"Canceled: Weekly standup".to_string())
    );
}

/// Cancelling an occurrence that already has an explicit override (RECURRENCE-ID component)
/// marks the existing override as cancelled instead of creating a second one.
#[tokio::test]
async fn cancel_existing_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "cancel-existing";
    write_recurring_with_override(&cal_dir, uid, false);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("rid", "TTEurope/Berlin;2026-04-15T09:00:00")]);
    let (status, _) = post_query(router, &format!("/api/items/cancel?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comps = ics.components();
    let override_comp = comps
        .iter()
        .find(|c| c.rid().is_some())
        .expect("expected a RECURRENCE-ID override");
    assert_eq!(
        override_comp.as_event().unwrap().status(),
        Some(CalEventStatus::Cancelled)
    );
}

/// Attempting to cancel an already-cancelled occurrence returns an error.
#[tokio::test]
async fn already_cancelled_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "cancel-already";
    write_recurring_with_override(&cal_dir, uid, true);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("rid", "TTEurope/Berlin;2026-04-15T09:00:00")]);
    let (status, body_str) = post_query(router, &format!("/api/items/cancel?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
    assert!(
        body_str.contains("already canceled"),
        "expected 'already canceled' error, got: {body_str}"
    );
}

/// Attempting to cancel a non-recurrent event (no RRULE) returns an error.
#[tokio::test]
async fn non_recurrent_event_rejected() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "cancel-non-recurrent";
    write_event_ics(&cal_dir, uid, "One-off event");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("rid", "TTEurope/Berlin;2026-04-15T09:00:00")]);
    let (status, _) = post_query(router, &format!("/api/items/cancel?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}

/// Attempting to cancel an event with an unknown UID returns an error.
#[tokio::test]
async fn unknown_uid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[
        ("uid", "no-such-uid"),
        ("rid", "TTEurope/Berlin;2026-04-15T09:00:00"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/cancel?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}
