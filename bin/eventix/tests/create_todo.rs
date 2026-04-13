// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod helper;

use chrono::NaiveDate;
use eventix_ical::objects::{CalDate, CalTodoStatus, EventLike};
use tempfile::TempDir;

use helper::create::{assert_success, read_created_ics};
use helper::{
    CAL_ID, assert_error, assert_no_ics, encode_form, first_component, make_router, make_state,
    merge_fields, post,
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

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Todo", &body).await;
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

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Todo", &body).await;
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

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Todo", &body).await;
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

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Todo", &body).await;
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

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Todo", &body).await;
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

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let todo = comp.as_todo().expect("expected VTODO component");
    assert_eq!(todo.status(), Some(CalTodoStatus::InProcess));
    assert_eq!(todo.percent(), Some(60), "expected PERCENT-COMPLETE:60");
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

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Todo", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

// --- Quick-add todo (API endpoint) ---

/// Quick-add a todo via POST /api/items/add. Verifies that a VTODO with the correct SUMMARY and
/// DUE date is produced.
#[tokio::test]
async fn api_quickadd_todo_basic() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let body = encode_form(&[
        ("quicktodo_calendar", CAL_ID),
        ("summary", "Buy milk"),
        ("due_date", "2026-04-20"),
    ]);

    let (status, _) = post(router, "/api/items/add", &body).await;
    assert_eq!(status, 200);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    assert_eq!(comp.summary(), Some(&"Buy milk".to_string()));

    let due_date = match comp.end_or_due().expect("expected DUE") {
        CalDate::Date(d, _) => *d,
        other => panic!("expected DUE as Date, got {:?}", other),
    };
    assert_eq!(due_date, NaiveDate::from_ymd_opt(2026, 4, 20).unwrap());
}
