// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::NaiveDate;
use eventix_ical::objects::{CalDate, EventLike};
use tempfile::TempDir;

use crate::helper::create::read_created_ics;
use crate::helper::{CAL_ID, encode_form, first_component, make_router, make_state, post};

// --- Quick-add todo via POST /api/items/add ---

/// Quick-add a todo with a summary and due date. Verifies that a VTODO with the correct SUMMARY
/// and an all-day DUE date is written to disk.
#[tokio::test]
async fn basic_with_due_date() {
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

/// Quick-add a todo without a due date. The VTODO should still be created with the correct
/// summary and no DUE property.
#[tokio::test]
async fn basic_without_due_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let body = encode_form(&[
        ("quicktodo_calendar", CAL_ID),
        ("summary", "Read a book"),
        ("due_date", ""),
    ]);

    let (status, _) = post(router, "/api/items/add", &body).await;
    assert_eq!(status, 200);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    assert_eq!(comp.summary(), Some(&"Read a book".to_string()));
    assert!(
        comp.end_or_due().is_none(),
        "expected no DUE property but got {:?}",
        comp.end_or_due()
    );
}
