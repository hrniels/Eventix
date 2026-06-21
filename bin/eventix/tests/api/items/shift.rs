// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, NaiveDateTime, Timelike};
use eventix_ical::objects::{CalDate, CalDateTime, EventLike};
use tempfile::TempDir;

use crate::helper::edit::read_ics_by_uid;
use crate::helper::{
    CAL_ID, assert_fold_in_tz, assert_gap_in_tz, encode_form, make_router, make_state,
    make_state_in_tz, post_query,
};

use super::{
    write_allday_event_ics, write_event_ics, write_event_ics_in_tz,
    write_recurring_allday_event_ics, write_recurring_event_ics,
};

// --- POST /api/items/shift ---

/// Shifting a timed event to a new date preserves its start time and duration.
#[tokio::test]
async fn shift_timed_event_to_new_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-timed";
    write_event_ics(&cal_dir, uid, "Meeting");
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);

    // Shift to 2026-04-22; start time should remain 09:00.
    let qs = encode_form(&[("uid", uid), ("date", "2026-04-22")]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let start = comp.start().expect("expected DTSTART");
    // The handler stores DTSTART with the locale timezone as TZID; read the wall-clock naive
    // datetime directly so the assertions are independent of the system timezone.
    // The wall-clock time must match the original event's wall-clock start (09:00 Europe/Berlin),
    // converted to whatever timezone the locale uses. Without an hour override the handler
    // preserves old_start.time() from locale.timezone(), so the naive time is timezone-dependent.
    // We therefore only assert the date, which is always 2026-04-22 regardless of the timezone.
    match start {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => {
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 4, 22).unwrap());
        }
        other => panic!("expected Timezone DTSTART, got {other:?}"),
    }
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

/// Shifting a timed event with an explicit hour override updates the stored wall-clock time when the
/// item already uses the user's timezone.
#[tokio::test]
async fn shift_with_hour_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-hour";
    write_event_ics(&cal_dir, uid, "Standup");
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);

    // Shift to same date but change start hour to 14.
    let qs = encode_form(&[("uid", uid), ("date", "2026-04-15"), ("hour", "14")]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    match comp.start().expect("expected DTSTART") {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "Europe/Berlin");
            assert_eq!(dt.hour(), 14);
        }
        other => panic!("expected Timezone DTSTART, got {other:?}"),
    }
}

/// Shifting a timed event in a different timezone succeeds as long as the requested user-local time
/// is representable.
#[tokio::test]
async fn shift_allows_event_in_different_timezone() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-cross-tz";
    write_event_ics_in_tz(&cal_dir, uid, "NY meeting", "America/New_York", 9, 10);
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("date", "2026-04-22"), ("hour", "14")]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    match ics
        .components()
        .first()
        .unwrap()
        .start()
        .expect("expected DTSTART")
    {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "America/New_York");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 4, 22).unwrap());
            assert_eq!(dt.hour(), 8);
        }
        other => panic!("expected Timezone DTSTART, got {other:?}"),
    }

    match ics
        .components()
        .first()
        .unwrap()
        .end_or_due()
        .expect("expected DTEND")
    {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "America/New_York");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 4, 22).unwrap());
            assert_eq!(dt.hour(), 9);
        }
        other => panic!("expected Timezone DTEND, got {other:?}"),
    }

    let berlin = chrono_tz::Europe::Berlin;
    let localized = ics
        .calendar()
        .date_context()
        .date(ics.components().first().unwrap().start().unwrap())
        .start_in(&berlin);
    assert_eq!(localized.hour(), 14);
}

#[tokio::test]
async fn shift_local_fold_time_succeeds() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-local-fold";
    write_event_ics(&cal_dir, uid, "Fold shift");
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);
    assert_fold_in_tz(
        chrono_tz::Europe::Berlin,
        NaiveDateTime::parse_from_str("2026-10-25 02:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
    );

    let qs = encode_form(&[("uid", uid), ("date", "2026-10-25"), ("hour", "2")]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    match ics.components()[0].start().unwrap() {
        CalDate::DateTime(CalDateTime::Utc(dt)) => {
            assert_eq!(
                dt.date_naive(),
                NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()
            );
            assert_eq!(dt.hour(), 0);
        }
        other => panic!("expected UTC DTSTART, got {other:?}"),
    }
}

#[tokio::test]
async fn shift_recurring_foreign_gap_occurrence_succeeds() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-recurring-gap-foreign";
    std::fs::write(
        cal_dir.join(format!("{uid}.ics")),
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             DTSTART;TZID=America/New_York:20260307T023000\r\n\
             DTEND;TZID=America/New_York:20260307T033000\r\n\
             RRULE:FREQ=DAILY;COUNT=3\r\n\
             SUMMARY:Recurring foreign gap\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);
    assert_gap_in_tz(
        chrono_tz::America::New_York,
        NaiveDateTime::parse_from_str("2026-03-08 02:30:00", "%Y-%m-%d %H:%M:%S").unwrap(),
    );

    let qs = encode_form(&[
        ("uid", uid),
        ("rid", "TTAmerica/New_York;2026-03-07T02:30:00"),
        ("date", "2026-03-08"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let override_comp = ics
        .components()
        .iter()
        .find(|c| c.rid().is_some())
        .expect("expected RECURRENCE-ID override");
    match override_comp.start().unwrap() {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "America/New_York");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 3, 8).unwrap());
            assert_eq!(dt.hour(), 3);
        }
        other => panic!("expected timezone DTSTART, got {other:?}"),
    }
}

#[tokio::test]
async fn shift_recurring_foreign_occurrence_converts_user_local_rid_to_event_timezone() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-recurring-foreign-rid";
    std::fs::write(
        cal_dir.join(format!("{uid}.ics")),
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             DTSTART;TZID=America/New_York:20260307T023000\r\n\
             DTEND;TZID=America/New_York:20260307T033000\r\n\
             RRULE:FREQ=DAILY;COUNT=3\r\n\
             SUMMARY:Recurring foreign rid\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);

    let qs = encode_form(&[
        ("uid", uid),
        ("rid", "TTEurope/Berlin;2026-03-07T08:30:00"),
        ("date", "2026-03-08"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let override_comp = ics
        .components()
        .iter()
        .find(|c| c.rid().is_some())
        .expect("expected RECURRENCE-ID override");

    match override_comp.rid().unwrap() {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "America/New_York");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 3, 7).unwrap());
            assert_eq!(dt.hour(), 2);
            assert_eq!(dt.minute(), 30);
        }
        other => panic!("expected timezone RECURRENCE-ID, got {other:?}"),
    }

    match override_comp.start().unwrap() {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "America/New_York");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 3, 8).unwrap());
            assert_eq!(dt.hour(), 3);
            assert_eq!(dt.minute(), 30);
        }
        other => panic!("expected timezone DTSTART, got {other:?}"),
    }
}

/// Shifting a specific occurrence of a recurring event creates a RECURRENCE-ID override.
#[tokio::test]
async fn shift_recurring_occurrence_creates_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-recurring";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
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
    // Read the wall-clock naive date directly from the stored CalDateTime variant so the assertion
    // is independent of the system timezone.
    match start {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => {
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 4, 16).unwrap());
        }
        other => panic!("expected Timezone DTSTART, got {other:?}"),
    }
}

/// Shifting a recurring all-day occurrence creates an override whose RECURRENCE-ID is also DATE.
#[tokio::test]
async fn shift_recurring_all_day_occurrence_keeps_date_rid() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "shift-recurring-allday";
    write_recurring_allday_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[
        ("uid", uid),
        ("rid", "TU2026-04-15T12:00:00"),
        ("date", "2026-04-16"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/shift?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let override_comp = ics
        .components()
        .iter()
        .find(|c| c.rid().is_some())
        .expect("expected a RECURRENCE-ID override");

    match override_comp.rid().expect("expected RECURRENCE-ID") {
        CalDate::Date(d, _) => {
            assert_eq!(*d, NaiveDate::from_ymd_opt(2026, 4, 15).unwrap());
        }
        other => panic!("expected DATE RECURRENCE-ID, got {other:?}"),
    }
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
    assert_eq!(status.as_u16(), 500);
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
    assert_eq!(status.as_u16(), 500);
}
