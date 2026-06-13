// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../../helper/mod.rs"]
mod helper;

use chrono::{NaiveDate, TimeZone, Timelike};
use eventix_ical::objects::{CalDate, CalDateTime, CalTodoStatus, EventLike};
use tempfile::TempDir;

use eventix_state::{CollectionSettings, SyncerType};
use helper::create::{assert_success, read_created_ics};
use helper::{
    CAL_ID, assert_error, assert_fold_in_tz, assert_gap_in_tz, assert_no_ics, encode_form,
    first_component, make_router, make_state, make_state_from_col, make_state_in_tz, merge_fields,
    post,
};

// --- Helpers specific to create-todo tests ---

/// Returns the set of form fields that every create-todo POST must include.
fn base_todo_fields<'a>() -> Vec<(&'a str, &'a str)> {
    vec![
        ("location", ""),
        ("description", ""),
        ("start_end[from][date]", ""),
        ("start_end[from][time]", ""),
        ("start_end[to][date]", ""),
        ("start_end[to][time]", ""),
        ("start_end[timezone]", "Europe/Berlin"),
        ("rrule[freq]", "NONE"),
        ("rrule[interval]", "1"),
        ("rrule[end]", "NoEnd"),
        ("rrule[count]", "1"),
        ("rrule[weekly_days]", ""),
        ("rrule[monthly_type]", "None"),
        ("rrule[yearly_type]", "None"),
        ("alarm[calendar][trigger]", "NONE"),
        ("alarm[calendar][duration]", "30"),
        ("alarm[calendar][durunit]", "Minutes"),
        ("alarm[calendar][durtype]", "BeforeStart"),
        ("status[status]", "NEEDS-ACTION"),
    ]
}

// --- Todos ---

/// A basic todo with summary only (no dates). Results in a VTODO with only SUMMARY.
#[tokio::test]
async fn todo_basic() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Buy groceries"),
            // No from/to dates or enabled flags → no DTSTART, no DUE
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    assert_eq!(comp.summary(), Some(&"Buy groceries".to_string()));
    assert!(comp.start().is_none(), "expected no DTSTART for basic todo");
    assert!(
        comp.end_or_due().is_none(),
        "expected no DUE for basic todo"
    );
}

/// A todo with a due date (date only). Results in DUE;VALUE=DATE.
#[tokio::test]
async fn todo_with_due_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "File tax return"),
            ("start_end[to][date]", "2026-04-30"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][durtype]", "BeforeEnd"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    assert!(comp.start().is_none(), "expected no DTSTART");
    let due_date = match comp.end_or_due().expect("expected DUE") {
        CalDate::Date(d, _) => *d,
        other => panic!("expected DUE as Date, got {:?}", other),
    };
    assert_eq!(due_date, NaiveDate::from_ymd_opt(2026, 4, 30).unwrap());
}

/// A todo with both start and due dates as timed datetimes.
#[tokio::test]
async fn todo_with_start_and_due() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Write report"),
            ("start_end[from][date]", "2026-05-01"),
            ("start_end[from][time]", "08:00"),
            ("start_end[to][date]", "2026-05-05"),
            ("start_end[to][time]", "17:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][durtype]", "BeforeEnd"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    match comp.start().expect("expected DTSTART") {
        CalDate::DateTime(_) => {}
        other => panic!("expected DTSTART as datetime, got {:?}", other),
    }
    match comp.end_or_due().expect("expected DUE") {
        CalDate::DateTime(_) => {}
        other => panic!("expected DUE as datetime, got {:?}", other),
    }
}

/// A todo with status NeedsAction. Verifies STATUS:NEEDS-ACTION in the output.
#[tokio::test]
async fn todo_status_needs_action() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Action item"),
            ("status[status]", "NEEDS-ACTION"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let todo = comp.as_todo().expect("expected VTODO component");
    assert_eq!(todo.status(), Some(CalTodoStatus::NeedsAction));
    assert!(todo.percent().is_none());
}

/// A todo with status Completed + a completion date. Verifies STATUS:COMPLETED,
/// COMPLETED property, and PERCENT-COMPLETE:100.
#[tokio::test]
async fn todo_status_completed() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Done task"),
            ("status[status]", "COMPLETED"),
            ("status[completed]", "2026-04-10"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let todo = comp.as_todo().expect("expected VTODO component");
    assert_eq!(todo.status(), Some(CalTodoStatus::Completed));
    assert_eq!(todo.percent(), Some(100), "expected PERCENT-COMPLETE:100");
    assert!(todo.completed().is_some(), "expected COMPLETED property");
}

/// A todo with status InProcess and a percent value. Verifies STATUS:IN-PROCESS and
/// PERCENT-COMPLETE.
#[tokio::test]
async fn todo_status_in_process() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Work in progress"),
            ("status[status]", "IN-PROCESS"),
            ("status[percent]", "60"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let todo = comp.as_todo().expect("expected VTODO component");
    assert_eq!(todo.status(), Some(CalTodoStatus::InProcess));
    assert_eq!(todo.percent(), Some(60), "expected PERCENT-COMPLETE:60");
}

/// A todo with a timed due date in UTC. Verifies the 'Z' suffix in the ICS file.
#[tokio::test]
async fn todo_with_utc_due() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "UTC Task"),
            ("start_end[to][date]", "2026-05-10"),
            ("start_end[to][time]", "10:00"),
            ("start_end[to_enabled]", "true"),
            ("start_end[timezone]", "UTC"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    // Verify the raw ICS content for the Z suffix
    let mut ics_path = cal_dir.clone();
    let entry = std::fs::read_dir(&cal_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    ics_path.push(entry.file_name());
    let content = std::fs::read_to_string(ics_path).unwrap();

    assert!(
        content.contains("DUE:20260510T100000Z"),
        "expected UTC Z suffix in DUE, but got:\n{content}"
    );
}

#[tokio::test]
async fn todo_with_foreign_dst_gap_due_is_accepted() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);
    assert_gap_in_tz(
        chrono_tz::America::New_York,
        NaiveDate::from_ymd_opt(2026, 3, 8)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap(),
    );

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Gap due todo"),
            ("start_end[to][date]", "2026-03-08"),
            ("start_end[to][time]", "02:30"),
            ("start_end[to_enabled]", "true"),
            ("start_end[timezone]", "America/New_York"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    match first_component(&ics).end_or_due().unwrap() {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "America/New_York");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 3, 8).unwrap());
            assert_eq!(dt.hour(), 2);
        }
        other => panic!("expected timezone DUE, got {other:?}"),
    }
}

#[tokio::test]
async fn todo_with_local_dst_gap_start_and_due_is_accepted() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);
    assert_gap_in_tz(
        chrono_tz::Europe::Berlin,
        NaiveDate::from_ymd_opt(2026, 3, 29)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap(),
    );

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Local gap todo"),
            ("start_end[from][date]", "2026-03-29"),
            ("start_end[from][time]", "02:30"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to][date]", "2026-03-29"),
            ("start_end[to][time]", "03:30"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][durtype]", "BeforeEnd"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    match comp.start().unwrap() {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "Europe/Berlin");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 3, 29).unwrap());
            assert_eq!(dt.hour(), 2);
        }
        other => panic!("expected timezone DTSTART, got {other:?}"),
    }
    match comp.end_or_due().unwrap() {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "Europe/Berlin");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 3, 29).unwrap());
            assert_eq!(dt.hour(), 3);
        }
        other => panic!("expected timezone DUE, got {other:?}"),
    }
}

#[tokio::test]
async fn todo_with_local_dst_fold_due_is_accepted() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);
    assert_fold_in_tz(
        chrono_tz::Europe::Berlin,
        NaiveDate::from_ymd_opt(2026, 10, 25)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap(),
    );

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Local fold todo"),
            ("start_end[from][date]", "2026-10-25"),
            ("start_end[from][time]", "01:30"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to][date]", "2026-10-25"),
            ("start_end[to][time]", "02:30"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][durtype]", "BeforeEnd"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    match first_component(&ics).end_or_due().unwrap() {
        CalDate::DateTime(CalDateTime::Timezone(dt, tzid)) => {
            assert_eq!(tzid, "Europe/Berlin");
            assert_eq!(dt.date(), NaiveDate::from_ymd_opt(2026, 10, 25).unwrap());
            assert_eq!(dt.hour(), 2);
            assert_eq!(dt.minute(), 30);
        }
        other => panic!("expected timezone DUE, got {other:?}"),
    }
}

#[tokio::test]
async fn recurring_todo_in_local_timezone_skips_gap_occurrence() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);
    assert_gap_in_tz(
        chrono_tz::Europe::Berlin,
        NaiveDate::from_ymd_opt(2026, 3, 29)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap(),
    );

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Recurring local gap todo"),
            ("start_end[from][date]", "2026-03-28"),
            ("start_end[from][time]", "02:30"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to][date]", "2026-03-28"),
            ("start_end[to][time]", "03:30"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][durtype]", "BeforeEnd"),
            ("rrule[freq]", "DAILY"),
            ("rrule[end]", "Count"),
            ("rrule[count]", "3"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let berlin = chrono_tz::Europe::Berlin;
    let starts: Vec<_> = ics
        .occurrences_between(
            berlin.with_ymd_and_hms(2026, 3, 27, 0, 0, 0).unwrap(),
            berlin.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
            |_| true,
        )
        .map(|occ| occ.occurrence_start().unwrap())
        .collect();
    assert_eq!(starts.len(), 2);
    assert_eq!(
        starts[0],
        berlin.with_ymd_and_hms(2026, 3, 28, 2, 30, 0).unwrap()
    );
    assert_eq!(
        starts[1],
        berlin.with_ymd_and_hms(2026, 3, 30, 2, 30, 0).unwrap()
    );
}

#[tokio::test]
async fn recurring_todo_in_foreign_timezone_keeps_first_fold_occurrence() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state_in_tz(&cal_dir, "Europe/Berlin");
    let router = make_router(state);
    assert_fold_in_tz(
        chrono_tz::America::New_York,
        NaiveDate::from_ymd_opt(2026, 11, 1)
            .unwrap()
            .and_hms_opt(1, 30, 0)
            .unwrap(),
    );

    let fields = merge_fields(
        base_todo_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Recurring foreign fold todo"),
            ("start_end[from][date]", "2026-10-31"),
            ("start_end[from][time]", "01:30"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to][date]", "2026-10-31"),
            ("start_end[to][time]", "02:30"),
            ("start_end[to_enabled]", "true"),
            ("start_end[timezone]", "America/New_York"),
            ("alarm[calendar][durtype]", "BeforeEnd"),
            ("rrule[freq]", "DAILY"),
            ("rrule[end]", "Count"),
            ("rrule[count]", "3"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let ny = chrono_tz::America::New_York;
    let starts: Vec<_> = ics
        .occurrences_between(
            ny.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).unwrap(),
            ny.with_ymd_and_hms(2026, 11, 3, 23, 59, 59).unwrap(),
            |_| true,
        )
        .map(|occ| occ.resolved_occurrence_start().unwrap().to_rfc3339())
        .collect();
    assert_eq!(starts.len(), 3);
    assert_eq!(starts[0], "2026-10-31T01:30:00-04:00");
    assert_eq!(starts[1], "2026-11-01T01:30:00-04:00");
    assert_eq!(starts[2], "2026-11-02T01:30:00-05:00");
}

/// A todo with missing summary is rejected.
#[tokio::test]
async fn todo_missing_summary() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(base_todo_fields(), &[("calendar", CAL_ID), ("summary", "")]);
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

/// A todo without any available calendars is rejected with a user-facing error banner.
#[tokio::test]
async fn todo_without_any_calendars() {
    let tmp = TempDir::new().unwrap();
    let col = CollectionSettings::new(SyncerType::FileSystem {
        path: tmp.path().to_string_lossy().into_owned(),
    });
    let (state, _settings_tmp) = make_state_from_col(col);
    let router = make_router(state);

    let fields = merge_fields(base_todo_fields(), &[("summary", "Buy groceries")]);
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/api/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert!(
        resp_body.contains("Please create a calendar first."),
        "expected missing-calendar message, got:\n{resp_body}"
    );
    assert_no_ics(tmp.path());
}
