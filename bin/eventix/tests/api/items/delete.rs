// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use tempfile::TempDir;

use crate::helper::{CAL_ID, encode_form, make_router, make_state, post_query};

use super::write_event_ics;

// --- POST /api/items/delete ---

/// Deleting an event by UID removes the ICS file from the calendar directory.
#[tokio::test]
async fn delete_removes_ics_file() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "delete-me";
    write_event_ics(&cal_dir, uid, "To be deleted");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid)]);
    let (status, _) = post_query(router, &format!("/api/items/delete?{qs}")).await;
    assert_eq!(status, 200);

    let ics_path = cal_dir.join(format!("{uid}.ics"));
    assert!(
        !ics_path.exists(),
        "ICS file should have been deleted but still exists"
    );
}

/// Deleting one of two events leaves the other intact.
#[tokio::test]
async fn delete_leaves_other_files_intact() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid_a = "delete-a";
    let uid_b = "delete-b";
    write_event_ics(&cal_dir, uid_a, "Event A");
    write_event_ics(&cal_dir, uid_b, "Event B");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid_a)]);
    let (status, _) = post_query(router, &format!("/api/items/delete?{qs}")).await;
    assert_eq!(status, 200);

    assert!(!cal_dir.join(format!("{uid_a}.ics")).exists());
    assert!(cal_dir.join(format!("{uid_b}.ics")).exists());
}
