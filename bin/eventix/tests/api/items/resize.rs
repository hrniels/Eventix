// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, Timelike};
use eventix_ical::objects::EventLike;
use tempfile::TempDir;

use crate::helper::edit::read_ics_by_uid;
use crate::helper::{CAL_ID, encode_form, make_router, make_state, post_query};

use super::{write_allday_event_ics, write_event_ics, write_recurring_event_ics};

// --- POST /api/items/resize ---

/// Resizing the end time of a simple timed event writes the new DTEND to disk.
#[tokio::test]
async fn resize_end_time() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-end";
    write_event_ics(&cal_dir, uid, "Meeting");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Original: 09:00–10:00. New end: 11:30.
    let qs = encode_form(&[("uid", uid), ("end_hour", "11"), ("end_minute", "30")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let end = comp.as_event().unwrap().end().expect("expected DTEND");
    let end_dt = end.as_start_with_tz(&chrono_tz::Europe::Berlin);
    assert_eq!(end_dt.hour(), 11);
    assert_eq!(end_dt.minute(), 30);
}

/// Resizing the start time of a simple timed event writes the new DTSTART to disk.
#[tokio::test]
async fn resize_start_time() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-start";
    write_event_ics(&cal_dir, uid, "Meeting");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Original: 09:00–10:00. New start: 08:30.
    let qs = encode_form(&[("uid", uid), ("start_hour", "8"), ("start_minute", "30")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let start = comp.start().expect("expected DTSTART");
    let start_dt = start.as_start_with_tz(&chrono_tz::Europe::Berlin);
    assert_eq!(start_dt.hour(), 8);
    assert_eq!(start_dt.minute(), 30);
}

/// Resizing a specific occurrence of a recurring event creates a RECURRENCE-ID override with the
/// new end time.
#[tokio::test]
async fn resize_recurring_occurrence_creates_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-recurring";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[
        ("uid", uid),
        ("rid", "TTEurope/Berlin;2026-04-15T09:00:00"),
        ("end_hour", "11"),
        ("end_minute", "0"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comps = ics.components();
    let override_comp = comps
        .iter()
        .find(|c| c.rid().is_some())
        .expect("expected a RECURRENCE-ID override");
    let end = override_comp
        .as_event()
        .unwrap()
        .end()
        .expect("expected DTEND");
    let end_dt = end.as_start_with_tz(&chrono_tz::Europe::Berlin);
    assert_eq!(end_dt.hour(), 11);
}

/// Supplying both start and end parameters at the same time returns an error.
#[tokio::test]
async fn both_start_and_end_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-both";
    write_event_ics(&cal_dir, uid, "Event");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[
        ("uid", uid),
        ("start_hour", "8"),
        ("start_minute", "0"),
        ("end_hour", "11"),
        ("end_minute", "0"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}

/// Supplying neither start nor end parameters returns an error.
#[tokio::test]
async fn neither_start_nor_end_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-neither";
    write_event_ics(&cal_dir, uid, "Event");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid)]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}

/// Supplying an invalid minute (not 0 or 30) returns an error.
#[tokio::test]
async fn invalid_minute_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-badmin";
    write_event_ics(&cal_dir, uid, "Event");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("end_hour", "11"), ("end_minute", "15")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}

/// Attempting to resize an all-day event returns an error.
#[tokio::test]
async fn all_day_event_rejected() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-allday";
    write_allday_event_ics(&cal_dir, uid, "All-day event");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("end_hour", "11"), ("end_minute", "0")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}

/// Resizing the end to the end-of-day midnight sentinel (hour=24, minute=0) sets DTEND to
/// midnight of the next day.
#[tokio::test]
async fn resize_end_midnight_sentinel() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-midnight";
    write_event_ics(&cal_dir, uid, "Late meeting");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // hour=24, minute=0 is the special sentinel for end-of-day midnight (next day).
    let qs = encode_form(&[("uid", uid), ("end_hour", "24"), ("end_minute", "0")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let end = comp.as_event().unwrap().end().expect("expected DTEND");
    let end_dt = end.as_start_with_tz(&chrono_tz::Europe::Berlin);
    // Midnight of next day: 2026-04-16 00:00.
    assert_eq!(end_dt.hour(), 0);
    assert_eq!(end_dt.minute(), 0);
    assert_eq!(
        end_dt.date_naive(),
        NaiveDate::from_ymd_opt(2026, 4, 16).unwrap()
    );
}

/// Resizing the start to a time after the existing end returns an error.
#[tokio::test]
async fn resize_start_after_end_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-start-late";
    write_event_ics(&cal_dir, uid, "Meeting");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Original end is 10:00; new start at 11:00 is after the end.
    let qs = encode_form(&[("uid", uid), ("start_hour", "11"), ("start_minute", "0")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}

/// Resizing the end to a time before the existing start returns an error.
#[tokio::test]
async fn resize_end_before_start_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-end-early";
    write_event_ics(&cal_dir, uid, "Meeting");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Original start is 09:00; new end at 08:30 is before the start.
    let qs = encode_form(&[("uid", uid), ("end_hour", "8"), ("end_minute", "30")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}
