// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod helper;

use chrono::NaiveDate;
use eventix_ical::objects::{CalDate, CalTodoStatus, EventLike};
use tempfile::TempDir;

use helper::edit::{assert_success, mtime_nanos, read_ics_by_uid};
use helper::{
    CAL_ID, assert_error, encode_form, first_component, make_router, make_state, merge_fields, post,
};

// --- Helpers specific to edit-todo tests ---

/// Writes a minimal VTODO ICS file for `uid` into `cal_dir` and returns the path.
fn write_todo_ics(cal_dir: &std::path::Path, uid: &str, summary: &str) -> std::path::PathBuf {
    let path = cal_dir.join(format!("{uid}.ics"));
    std::fs::write(
        &path,
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
    path
}

/// Writes a VTODO with a due date and a status into `cal_dir`.
fn write_todo_ics_with_due(
    cal_dir: &std::path::Path,
    uid: &str,
    summary: &str,
    due: &str,
) -> std::path::PathBuf {
    let path = cal_dir.join(format!("{uid}.ics"));
    std::fs::write(
        &path,
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VTODO\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             DUE;VALUE=DATE:{due}\r\n\
             SUMMARY:{summary}\r\n\
             STATUS:NEEDS-ACTION\r\n\
             END:VTODO\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
    path
}

/// Returns the base form fields every edit-todo POST must include.
fn base_edit_todo_fields(edit_start: &str) -> Vec<(&str, &str)> {
    vec![
        ("edit_start", edit_start),
        ("calendar", CAL_ID),
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

// --- Series edits ---

/// Editing the summary of a basic todo (Series mode).
#[tokio::test]
async fn series_edit_todo_summary() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-todo-summary";
    let ics_path = write_todo_ics(&cal_dir, uid, "Buy milk");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_todo_fields(&edit_start),
        &[("summary", "Buy oat milk")],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = first_component(&ics);
    assert_eq!(comp.summary(), Some(&"Buy oat milk".to_string()));
}

/// Adding a due date to a todo that previously had none.
#[tokio::test]
async fn series_edit_todo_add_due_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-todo-due";
    let ics_path = write_todo_ics(&cal_dir, uid, "File taxes");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_todo_fields(&edit_start),
        &[
            ("summary", "File taxes"),
            ("start_end[to][date]", "2026-04-30"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][durtype]", "BeforeEnd"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = first_component(&ics);
    let due = match comp.end_or_due().expect("expected DUE") {
        CalDate::Date(d, _) => d,
        other => panic!("expected DUE as Date, got {:?}", other),
    };
    assert_eq!(*due, NaiveDate::from_ymd_opt(2026, 4, 30).unwrap());
}

/// Changing the status from NeedsAction to InProcess and setting a percent.
#[tokio::test]
async fn series_edit_todo_status_in_process() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-todo-in-process";
    let ics_path = write_todo_ics_with_due(&cal_dir, uid, "Write report", "20260501");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_todo_fields(&edit_start),
        &[
            ("summary", "Write report"),
            ("start_end[to][date]", "2026-05-01"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][durtype]", "BeforeEnd"),
            ("status[status]", "IN-PROCESS"),
            ("status[percent]", "40"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = first_component(&ics);
    let todo = comp.as_todo().expect("expected VTODO");
    assert_eq!(todo.status(), Some(CalTodoStatus::InProcess));
    assert_eq!(todo.percent(), Some(40));
}

/// Changing the status to Completed with a completion date.
#[tokio::test]
async fn series_edit_todo_status_completed() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-todo-completed";
    let ics_path = write_todo_ics(&cal_dir, uid, "Send invoice");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_todo_fields(&edit_start),
        &[
            ("summary", "Send invoice"),
            ("status[status]", "COMPLETED"),
            ("status[completed]", "2026-04-10"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = first_component(&ics);
    let todo = comp.as_todo().expect("expected VTODO");
    assert_eq!(todo.status(), Some(CalTodoStatus::Completed));
    assert_eq!(todo.percent(), Some(100));
    assert!(todo.completed().is_some());
}

/// Changing the status to Cancelled.
#[tokio::test]
async fn series_edit_todo_status_cancelled() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-todo-cancelled";
    let ics_path = write_todo_ics(&cal_dir, uid, "Old task");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_todo_fields(&edit_start),
        &[("summary", "Old task"), ("status[status]", "CANCELLED")],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = first_component(&ics);
    let todo = comp.as_todo().expect("expected VTODO");
    assert_eq!(todo.status(), Some(CalTodoStatus::Cancelled));
    assert!(todo.percent().is_none());
}

/// Adding a location and description to a previously minimal todo.
#[tokio::test]
async fn series_edit_todo_location_and_description() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-todo-loc-desc";
    let ics_path = write_todo_ics(&cal_dir, uid, "Dentist");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_todo_fields(&edit_start),
        &[
            ("summary", "Dentist"),
            ("location", "Main St Dental"),
            ("description", "Bring insurance card"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = first_component(&ics);
    assert_eq!(comp.location(), Some(&"Main St Dental".to_string()));
    assert_eq!(
        comp.description(),
        Some(&"Bring insurance card".to_string())
    );
}

// --- Error paths ---

/// An edit with edit_start = 0 is rejected due to staleness.
#[tokio::test]
async fn series_edit_todo_stale_rejected() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-todo-stale";
    write_todo_ics(&cal_dir, uid, "Stale todo");

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_todo_fields("0"),
        &[("summary", "Should not save")],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
}

/// An edit with an empty summary is rejected.
#[tokio::test]
async fn series_edit_todo_missing_summary() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-todo-no-summary";
    let ics_path = write_todo_ics(&cal_dir, uid, "Has summary");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(base_edit_todo_fields(&edit_start), &[("summary", "")]);
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
}
