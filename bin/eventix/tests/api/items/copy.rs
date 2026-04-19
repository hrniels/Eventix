// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, Timelike};
use eventix_ical::objects::{CalDate, CalDateTime, EventLike};
use tempfile::TempDir;

use crate::helper::{
    CAL_ID, encode_form, first_component, make_router, make_state, make_state_in_tz, post_query,
};

use super::{
    write_allday_event_ics, write_event_ics, write_event_ics_in_tz, write_recurring_event_ics,
};

// --- POST /api/items/copy ---

/// Copying a timed event to a new date creates a new ICS file with a fresh UID, the same summary,
/// and an updated DTSTART/DTEND on the target date.
#[tokio::test]
async fn copy_timed_event_to_new_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "copy-timed";
    write_event_ics(&cal_dir, uid, "Team meeting");
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);

    // Copy to 2026-04-22 (same start time).
    let qs = encode_form(&[("uid", uid), ("date", "2026-04-22")]);
    let (status, _) = post_query(router, &format!("/api/items/copy?{qs}")).await;
    assert_eq!(status, 200);

    // Two ICS files should now exist: the original and the copy. Find the copy by scanning for
    // the file that is NOT the original uid.
    let entries: Vec<_> = std::fs::read_dir(&cal_dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("ics")
                && p.file_stem().and_then(|s| s.to_str()) != Some(uid)
            {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(entries.len(), 1, "expected exactly 1 copy ICS file");

    let tz = chrono_tz::UTC;
    let copy_ics = eventix_ical::col::CalFile::new_from_file(
        std::sync::Arc::new(CAL_ID.to_string()),
        entries[0].clone(),
        &tz,
    )
    .unwrap();
    let comp = first_component(&copy_ics);
    assert_eq!(comp.summary(), Some(&"Team meeting".to_string()));

    // Verify that start is on 2026-04-22. Read the wall-clock naive date from the stored
    // CalDateTime variant directly so the assertion is independent of the system timezone.
    let start = comp.start().expect("copy must have DTSTART");
    match start {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => {
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 4, 22).unwrap());
        }
        other => panic!("expected Timezone DTSTART, got {other:?}"),
    }
}

/// Copying a timed event with an explicit hour override updates the stored wall-clock time when the
/// item already uses the user's timezone.
#[tokio::test]
async fn copy_with_hour_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "copy-hour";
    write_event_ics(&cal_dir, uid, "Standup");
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);

    // Copy to same date but shift start to 14:00.
    let qs = encode_form(&[("uid", uid), ("date", "2026-04-22"), ("hour", "14")]);
    let (status, _) = post_query(router, &format!("/api/items/copy?{qs}")).await;
    assert_eq!(status, 200);

    let entries: Vec<_> = std::fs::read_dir(&cal_dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("ics")
                && p.file_stem().and_then(|s| s.to_str()) != Some(uid)
            {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(entries.len(), 1);

    let tz = chrono_tz::UTC;
    let copy_ics = eventix_ical::col::CalFile::new_from_file(
        std::sync::Arc::new(CAL_ID.to_string()),
        entries[0].clone(),
        &tz,
    )
    .unwrap();
    let comp = first_component(&copy_ics);
    match comp.start().expect("copy must have DTSTART") {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "Europe/Berlin");
            assert_eq!(dt.hour(), 14);
        }
        other => panic!("expected Timezone DTSTART, got {other:?}"),
    }
}

/// Copying a timed event in a different timezone succeeds as long as the requested user-local time
/// is representable.
#[tokio::test]
async fn copy_allows_event_in_different_timezone() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "copy-cross-tz";
    write_event_ics_in_tz(&cal_dir, uid, "NY meeting", "America/New_York", 9, 10);
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("date", "2026-04-22"), ("hour", "14")]);
    let (status, _) = post_query(router, &format!("/api/items/copy?{qs}")).await;
    assert_eq!(status, 200);

    let entries: Vec<_> = std::fs::read_dir(&cal_dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("ics")
                && p.file_stem().and_then(|s| s.to_str()) != Some(uid)
            {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(entries.len(), 1);

    let tz = chrono_tz::UTC;
    let copy_ics = eventix_ical::col::CalFile::new_from_file(
        std::sync::Arc::new(CAL_ID.to_string()),
        entries[0].clone(),
        &tz,
    )
    .unwrap();
    let comp = first_component(&copy_ics);

    match comp.start().expect("copy must have DTSTART") {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "America/New_York");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 4, 22).unwrap());
            assert_eq!(dt.hour(), 8);
        }
        other => panic!("expected Timezone DTSTART, got {other:?}"),
    }

    match comp.end_or_due().expect("copy must have DTEND") {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "America/New_York");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 4, 22).unwrap());
            assert_eq!(dt.hour(), 9);
        }
        other => panic!("expected Timezone DTEND, got {other:?}"),
    }

    let berlin = chrono_tz::Europe::Berlin;
    let localized = copy_ics
        .calendar()
        .date_context()
        .date(comp.start().unwrap())
        .start_in(&berlin);
    assert_eq!(localized.hour(), 14);
}

/// Copying an event that uses an embedded custom `VTIMEZONE` still fails when the requested
/// user-local time falls into the local DST gap.
#[tokio::test]
async fn copy_rejects_embedded_vtimezone_in_user_local_dst_gap() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "copy-custom-dst";
    std::fs::write(
        cal_dir.join(format!("{uid}.ics")),
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VTIMEZONE\r\n\
             TZID:X-CUSTOM-DST\r\n\
             BEGIN:STANDARD\r\n\
             DTSTART:19700101T000000\r\n\
             TZOFFSETFROM:+0200\r\n\
             TZOFFSETTO:+0100\r\n\
             TZNAME:CST\r\n\
             END:STANDARD\r\n\
             BEGIN:DAYLIGHT\r\n\
             DTSTART:20250330T040000\r\n\
             TZOFFSETFROM:+0100\r\n\
             TZOFFSETTO:+0200\r\n\
             TZNAME:CDT\r\n\
             END:DAYLIGHT\r\n\
             END:VTIMEZONE\r\n\
             BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20250101T000000Z\r\n\
             DTSTART;TZID=X-CUSTOM-DST:20250329T090000\r\n\
             DTEND;TZID=X-CUSTOM-DST:20250329T100000\r\n\
             SUMMARY:Custom DST copy\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();

    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("date", "2025-03-30"), ("hour", "2")]);
    let (status, _) = post_query(router, &format!("/api/items/copy?{qs}")).await;
    assert_eq!(status.as_u16(), 100);

    let count = std::fs::read_dir(&cal_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .and_then(|s| s.to_str())
                == Some("ics")
        })
        .count();
    assert_eq!(count, 1);
}

/// Copying to a user-local time that falls into a DST gap fails because the copied item would not
/// be representable in the user's calendar view.
#[tokio::test]
async fn copy_rejects_user_local_dst_gap() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "copy-dst-gap";
    write_event_ics_in_tz(&cal_dir, uid, "NY meeting", "America/New_York", 9, 10);
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("date", "2026-03-29"), ("hour", "2")]);
    let (status, _) = post_query(router, &format!("/api/items/copy?{qs}")).await;
    assert_eq!(status.as_u16(), 100);

    let count = std::fs::read_dir(&cal_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .and_then(|s| s.to_str())
                == Some("ics")
        })
        .count();
    assert_eq!(count, 1);
}

/// Attempting to copy a recurrent event returns an error.
#[tokio::test]
async fn recurrent_event_rejected() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "copy-recurrent";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("date", "2026-04-22")]);
    let (status, _) = post_query(router, &format!("/api/items/copy?{qs}")).await;
    assert_eq!(status.as_u16(), 100);

    // No copy should have been created.
    let count = std::fs::read_dir(&cal_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .and_then(|s| s.to_str())
                == Some("ics")
        })
        .count();
    assert_eq!(count, 1, "copy of recurrent event must not be created");
}

/// Supplying an unknown UID returns an error.
#[tokio::test]
async fn unknown_uid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    // Ensure there is at least one ICS present so the router can load state.
    write_event_ics(&cal_dir, "some-uid", "Event");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", "no-such-uid"), ("date", "2026-04-22")]);
    let (status, _) = post_query(router, &format!("/api/items/copy?{qs}")).await;
    assert_eq!(status.as_u16(), 100);

    // Only the original ICS should exist.
    let count = std::fs::read_dir(&cal_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .and_then(|s| s.to_str())
                == Some("ics")
        })
        .count();
    assert_eq!(count, 1);
}

/// Copying an all-day event to a new date creates a copy with the DATE value on the target day.
#[tokio::test]
async fn copy_allday_event_to_new_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "copy-allday";
    write_allday_event_ics(&cal_dir, uid, "Birthday");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("date", "2026-05-01")]);
    let (status, _) = post_query(router, &format!("/api/items/copy?{qs}")).await;
    assert_eq!(status, 200);

    // Find the newly created copy.
    let entries: Vec<_> = std::fs::read_dir(&cal_dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("ics")
                && p.file_stem().and_then(|s| s.to_str()) != Some(uid)
            {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(entries.len(), 1, "expected exactly 1 copy ICS file");

    let tz = chrono_tz::UTC;
    let copy_ics = eventix_ical::col::CalFile::new_from_file(
        std::sync::Arc::new(CAL_ID.to_string()),
        entries[0].clone(),
        &tz,
    )
    .unwrap();
    let comp = first_component(&copy_ics);
    let start = comp.start().expect("copy must have DTSTART");
    match start {
        CalDate::Date(d, _) => {
            assert_eq!(*d, NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
        }
        other => panic!("expected DATE start for all-day copy, got {other:?}"),
    }
}
