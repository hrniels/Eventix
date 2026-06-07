// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::NaiveDate;
use std::path::Path;

use tempfile::TempDir;

use crate::helper::edit::read_ics_by_uid;
use crate::helper::{CAL_ID, encode_form, make_router, make_state, post_query};

/// Writes a VTODO ICS file for `uid` into `cal_dir` with a DUE DATE (no time).
fn write_todo_with_date_due_ics(cal_dir: &Path, uid: &str, summary: &str, due_date: &str) {
    std::fs::write(
        cal_dir.join(format!("{uid}.ics")),
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VTODO\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             DUE;VALUE=DATE:{}\r\n\
             SUMMARY:{summary}\r\n\
             END:VTODO\r\n\
             END:VCALENDAR\r\n",
            due_date
        ),
    )
    .unwrap();
}

// --- POST /api/items/postpone ---

/// Postponing a simple VTODO with a due date shifts the due date by the requested number of days.
#[tokio::test]
async fn postpone_todo_basic() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "postpone-basic";
    write_todo_with_date_due_ics(&cal_dir, uid, "Pay rent", "20260415");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("delay_days", "7")]);
    let (status, _) = post_query(router, &format!("/api/items/postpone?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let todo = ics.components().first().unwrap().as_todo().unwrap();
    let due = todo.due().expect("due date should be set");
    assert_eq!(
        due.as_naive_date(),
        NaiveDate::from_ymd_opt(2026, 4, 22).unwrap()
    );
}

/// Postponing a VTODO without a due date returns an error.
#[tokio::test]
async fn postpone_todo_without_due_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "postpone-no-due";

    std::fs::write(
        cal_dir.join(format!("{uid}.ics")),
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VTODO\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             SUMMARY:No due date task\r\n\
             END:VTODO\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("delay_days", "1")]);
    let (status, _) = post_query(router, &format!("/api/items/postpone?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}
