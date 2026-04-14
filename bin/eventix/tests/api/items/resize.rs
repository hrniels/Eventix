// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};
use eventix_ical::objects::{CalDate, CalDateTime, EventLike};
use tempfile::TempDir;

use crate::helper::edit::read_ics_by_uid;
use crate::helper::{CAL_ID, encode_form, make_router, make_state, post_query};

use super::write_allday_event_ics;

// --- POST /api/items/resize ---

/// Writes a timed VEVENT ICS for `uid` into `cal_dir` using the given `start` and `end` dates.
///
/// When `rrule` is `Some`, the rule is inserted as an `RRULE` property, producing a recurring
/// event. Pass `None` for a plain single-occurrence event.
fn write_timed_event_ics(
    cal_dir: &Path,
    uid: &str,
    summary: &str,
    start: &CalDate,
    end: &CalDate,
    rrule: Option<&str>,
) {
    let rrule_line = match rrule {
        Some(rule) => format!("RRULE:{rule}\r\n"),
        None => String::new(),
    };
    let path = cal_dir.join(format!("{uid}.ics"));
    std::fs::write(
        &path,
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             {}\r\n\
             {}\r\n\
             {rrule_line}\
             SUMMARY:{summary}\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n",
            start.to_prop("DTSTART"),
            end.to_prop("DTEND"),
        ),
    )
    .unwrap();
}

/// Returns a `CalDate` for 2026-04-15 at the given hour:minute in the given TZID string.
fn in_tz(hour: u32, minute: u32, tzid: &str) -> CalDate {
    CalDate::DateTime(CalDateTime::Timezone(
        NaiveDateTime::new(
            NaiveDate::from_ymd_opt(2026, 4, 15).unwrap(),
            NaiveTime::from_hms_opt(hour, minute, 0).unwrap(),
        ),
        tzid.to_string(),
    ))
}

/// Returns the system timezone name as reported by the OS.
fn locale_tz() -> String {
    iana_time_zone::get_timezone().unwrap()
}

/// Resizing the end time of an event stored in the **locale timezone** writes the new DTEND to
/// disk.
#[tokio::test]
async fn resize_end_time() {
    let locale_tz = locale_tz();
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-end";
    write_timed_event_ics(
        &cal_dir,
        uid,
        "Meeting",
        &in_tz(9, 0, &locale_tz),
        &in_tz(10, 0, &locale_tz),
        None,
    );
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Original: 09:00–10:00. New end: 11:30.
    let qs = encode_form(&[("uid", uid), ("end_hour", "11"), ("end_minute", "30")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let end = comp.as_event().unwrap().end().expect("expected DTEND");
    // Read the wall-clock naive time directly so the assertion is independent of the system
    // timezone.
    match end {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => {
            assert_eq!(dt.hour(), 11);
            assert_eq!(dt.minute(), 30);
        }
        other => panic!("expected Timezone DTEND, got {other:?}"),
    }
}

/// Resizing the start time of an event stored in **UTC** (a timezone-neutral representation
/// that is always different from any locale timezone) writes the new DTSTART to disk.
///
/// This exercises the cross-timezone conversion path in the handler. The event is stored at
/// a fixed UTC instant (14:00–15:00 UTC on 2026-04-15). At runtime the locale timezone is
/// read to compute a new start time that is guaranteed to be before the event's end in every
/// possible system timezone.
#[tokio::test]
async fn resize_start_time() {
    // Fixed UTC instant: 14:00–15:00 UTC on 2026-04-15. In every real-world timezone this
    // maps to a local time whose hour is in [0, 23]; when we request a new start of
    // (old_start_h - 1):30 (clamped to 00:30 at minimum) it is always strictly before the
    // local end time because the event window is a full hour.
    let start_utc = NaiveDate::from_ymd_opt(2026, 4, 15)
        .unwrap()
        .and_hms_opt(14, 0, 0)
        .unwrap()
        .and_utc();
    let end_utc = NaiveDate::from_ymd_opt(2026, 4, 15)
        .unwrap()
        .and_hms_opt(15, 0, 0)
        .unwrap()
        .and_utc();

    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-start";
    write_timed_event_ics(
        &cal_dir,
        uid,
        "Meeting",
        &CalDate::DateTime(CalDateTime::Utc(start_utc)),
        &CalDate::DateTime(CalDateTime::Utc(end_utc)),
        None,
    );
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Determine the new start hour from the locale timezone at runtime so the resize request
    // is always valid regardless of the system timezone.
    let locale_tz: chrono_tz::Tz = iana_time_zone::get_timezone()
        .unwrap()
        .parse()
        .unwrap_or(chrono_tz::UTC);
    let old_start_local = locale_tz.from_utc_datetime(&start_utc.naive_utc());
    let new_h = old_start_local.hour().saturating_sub(1);

    let qs = encode_form(&[
        ("uid", uid),
        ("start_hour", &new_h.to_string()),
        ("start_minute", "30"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let start = comp.start().expect("expected DTSTART");
    // The handler rewrites DTSTART as CalDateTime::Timezone with the locale TZID and the
    // requested wall-clock naive time. Read the naive time directly so the assertion is
    // independent of which timezone the locale is.
    match start {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => {
            assert_eq!(dt.hour(), new_h);
            assert_eq!(dt.minute(), 30);
        }
        other => panic!("expected Timezone DTSTART, got {other:?}"),
    }
}

/// Resizing the end of a specific occurrence of a recurring event stored in **Europe/Berlin**
/// creates a RECURRENCE-ID override with the new end time.
///
/// Europe/Berlin is required here so that the `rid` query parameter
/// (`TTEurope/Berlin;2026-04-15T09:00:00`) matches the stored DTSTART TZID.
#[tokio::test]
async fn resize_recurring_occurrence_creates_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-recurring";
    write_timed_event_ics(
        &cal_dir,
        uid,
        "Weekly standup",
        &in_tz(9, 0, "Europe/Berlin"),
        &in_tz(10, 0, "Europe/Berlin"),
        Some("FREQ=WEEKLY;BYDAY=WE"),
    );
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
    // Read the wall-clock naive time directly so the assertion is independent of the system
    // timezone.
    match end {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => {
            assert_eq!(dt.hour(), 11);
        }
        other => panic!("expected Timezone DTEND, got {other:?}"),
    }
}

/// Supplying both start and end parameters at the same time returns an error.
#[tokio::test]
async fn both_start_and_end_returns_error() {
    let locale_tz = locale_tz();
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-both";
    write_timed_event_ics(
        &cal_dir,
        uid,
        "Event",
        &in_tz(9, 0, &locale_tz),
        &in_tz(10, 0, &locale_tz),
        None,
    );
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
    let locale_tz = locale_tz();
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-neither";
    write_timed_event_ics(
        &cal_dir,
        uid,
        "Event",
        &in_tz(9, 0, &locale_tz),
        &in_tz(10, 0, &locale_tz),
        None,
    );
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid)]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}

/// Supplying an invalid minute (not 0 or 30) returns an error.
#[tokio::test]
async fn invalid_minute_returns_error() {
    let locale_tz = locale_tz();
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-badmin";
    write_timed_event_ics(
        &cal_dir,
        uid,
        "Event",
        &in_tz(9, 0, &locale_tz),
        &in_tz(10, 0, &locale_tz),
        None,
    );
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
    let locale_tz = locale_tz();
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-midnight";
    write_timed_event_ics(
        &cal_dir,
        uid,
        "Late meeting",
        &in_tz(9, 0, &locale_tz),
        &in_tz(10, 0, &locale_tz),
        None,
    );
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // hour=24, minute=0 is the special sentinel for end-of-day midnight (next day).
    let qs = encode_form(&[("uid", uid), ("end_hour", "24"), ("end_minute", "0")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let end = comp.as_event().unwrap().end().expect("expected DTEND");
    // The midnight sentinel stores a wall-clock NaiveDateTime of 00:00:00 on the next day.
    // Read the Timezone variant directly so the assertion is independent of the system timezone.
    match end {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => {
            assert_eq!(
                *dt,
                NaiveDateTime::new(
                    NaiveDate::from_ymd_opt(2026, 4, 16).unwrap(),
                    NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                )
            );
        }
        other => panic!("expected Timezone DTEND, got {other:?}"),
    }
}

/// Resizing the start to a time after the existing end returns an error.
#[tokio::test]
async fn resize_start_after_end_returns_error() {
    let locale_tz = locale_tz();
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-start-late";
    write_timed_event_ics(
        &cal_dir,
        uid,
        "Meeting",
        &in_tz(9, 0, &locale_tz),
        &in_tz(10, 0, &locale_tz),
        None,
    );
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // New start 11:00 is after the end (10:00 wall clock) in every timezone.
    let qs = encode_form(&[("uid", uid), ("start_hour", "11"), ("start_minute", "0")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}

/// Resizing the end to a time before the existing start returns an error.
#[tokio::test]
async fn resize_end_before_start_returns_error() {
    let locale_tz = locale_tz();
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "resize-end-early";
    write_timed_event_ics(
        &cal_dir,
        uid,
        "Meeting",
        &in_tz(9, 0, &locale_tz),
        &in_tz(10, 0, &locale_tz),
        None,
    );
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // New end 08:30 is before the start (09:00 wall clock) in every timezone.
    let qs = encode_form(&[("uid", uid), ("end_hour", "8"), ("end_minute", "30")]);
    let (status, _) = post_query(router, &format!("/api/items/resize?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}
