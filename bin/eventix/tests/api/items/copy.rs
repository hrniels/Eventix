// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, Timelike};
use eventix_ical::objects::{CalDate, CalDateTime, EventLike};
use tempfile::TempDir;

use crate::helper::{CAL_ID, encode_form, first_component, make_router, make_state, post_query};

use super::{write_allday_event_ics, write_event_ics, write_recurring_event_ics};

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
    let state = make_state(&cal_dir);
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

/// Copying a timed event with an explicit hour override changes the start hour of the copy while
/// preserving the original duration.
#[tokio::test]
async fn copy_with_hour_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "copy-hour";
    write_event_ics(&cal_dir, uid, "Standup");
    let state = make_state(&cal_dir);
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
    let start = comp.start().expect("copy must have DTSTART");
    // The handler stores DTSTART with the locale timezone as TZID; read the wall-clock naive time
    // directly from the CalDateTime variant so the assertion is independent of the system timezone.
    match start {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => {
            assert_eq!(dt.hour(), 14);
        }
        other => panic!("expected Timezone DTSTART, got {other:?}"),
    }
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
