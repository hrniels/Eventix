// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, Timelike};
use eventix_ical::objects::{CalDate, EventLike};
use tempfile::TempDir;

use crate::helper::edit::read_ics_by_uid;
use crate::helper::{CAL_ID, encode_form, make_router, make_state, post_query};

use super::{write_allday_event_ics, write_event_ics, write_recurring_event_ics};

// --- POST /api/items/shift ---

/// Shifting a timed event to a new date preserves its start time and duration.
#[tokio::test]
async fn shift_timed_event_to_new_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-timed";
    write_event_ics(&cal_dir, uid, "Meeting");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Shift to 2026-04-22; start time should remain 09:00.
    let qs = encode_form(&[("uid", uid), ("date", "2026-04-22")]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let start = comp.start().expect("expected DTSTART");
    let start_dt = start.as_start_with_tz(&chrono_tz::Europe::Berlin);
    assert_eq!(
        start_dt.date_naive(),
        NaiveDate::from_ymd_opt(2026, 4, 22).unwrap()
    );
    assert_eq!(start_dt.hour(), 9);
    assert_eq!(start_dt.minute(), 0);
}

/// Shifting an all-day event to a new date updates the DATE value.
#[tokio::test]
async fn shift_all_day_event() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-allday";
    write_allday_event_ics(&cal_dir, uid, "Birthday");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("date", "2026-05-01")]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let start = comp.start().expect("expected DTSTART");
    match start {
        CalDate::Date(d, _) => {
            assert_eq!(*d, NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
        }
        other => panic!("expected DATE start, got {other:?}"),
    }
}

/// Shifting a timed event with an explicit hour override changes the start hour.
#[tokio::test]
async fn shift_with_hour_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-hour";
    write_event_ics(&cal_dir, uid, "Standup");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Shift to same date but change start hour to 14.
    let qs = encode_form(&[("uid", uid), ("date", "2026-04-15"), ("hour", "14")]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let start = comp.start().expect("expected DTSTART");
    let start_dt = start.as_start_with_tz(&chrono_tz::Europe::Berlin);
    assert_eq!(start_dt.hour(), 14);
}

/// Shifting a specific occurrence of a recurring event creates a RECURRENCE-ID override.
#[tokio::test]
async fn shift_recurring_occurrence_creates_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-recurring";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[
        ("uid", uid),
        ("rid", "TTEurope/Berlin;2026-04-15T09:00:00"),
        ("date", "2026-04-16"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comps = ics.components();
    let override_comp = comps
        .iter()
        .find(|c| c.rid().is_some())
        .expect("expected a RECURRENCE-ID override");
    let start = override_comp.start().expect("expected DTSTART");
    let start_dt = start.as_start_with_tz(&chrono_tz::Europe::Berlin);
    assert_eq!(
        start_dt.date_naive(),
        NaiveDate::from_ymd_opt(2026, 4, 16).unwrap()
    );
}

/// Supplying an unknown UID returns an error.
#[tokio::test]
async fn unknown_uid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "other", "Other event");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", "no-such-uid"), ("date", "2026-04-22")]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}

/// Supplying a `rid` for a non-recurrent event (no matching component and non-recurrent base)
/// returns an error — the handler rejects the operation when the base is not recurrent.
#[tokio::test]
async fn shift_non_recurrent_with_rid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-non-recur";
    write_event_ics(&cal_dir, uid, "Single event");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // A rid that doesn't match any override forces the `else` branch; the non-recurrent base then
    // triggers the "Component is not recurrent" error.
    let qs = encode_form(&[
        ("uid", uid),
        ("rid", "TTEurope/Berlin;2026-04-15T09:00:00"),
        ("date", "2026-04-22"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}
