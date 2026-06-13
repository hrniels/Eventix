// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use tempfile::TempDir;

use crate::helper::{CAL_ID, get_raw, make_router, make_state};

use super::write_event_ics;

// --- GET /api/items/dstwarn ---

#[tokio::test]
async fn returns_gap_hit_for_range_spanning_spring_forward() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "dummy", "Dummy");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = "/api/items/dstwarn\
               ?from_date=2026-10-04&from_time=01:45\
               &to_date=2026-10-04&to_time=03:00\
               &timezone=Australia%2FLord_Howe";
    let (status, body) = get_raw(router, uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        json["warning"]
            .as_str()
            .unwrap()
            .contains("DST transition (2026-10-04 02:00)")
    );
}

#[tokio::test]
async fn returns_fold_hit_for_range_spanning_fall_back() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "dummy", "Dummy");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = "/api/items/dstwarn\
               ?from_date=2025-04-06&from_time=02:30\
               &to_date=2025-04-06&to_time=04:00\
               &timezone=Pacific%2FChatham";
    let (status, body) = get_raw(router, uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        json["warning"]
            .as_str()
            .unwrap()
            .contains("DST transition (2025-04-06 02:45)")
    );
}

#[tokio::test]
async fn returns_null_without_transition() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "dummy", "Dummy");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = "/api/items/dstwarn\
               ?from_date=2026-05-15&from_time=09:00\
               &to_date=2026-05-15&to_time=10:00\
               &timezone=Pacific%2FChatham";
    let (status, body) = get_raw(router, uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["warning"].is_null());
}

#[tokio::test]
async fn returns_null_for_incomplete_fields() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "dummy", "Dummy");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = "/api/items/dstwarn\
               ?from_date=2026-03-29&from_time=01:30\
               &to_date=2026-03-29&to_time=10:\
               &timezone=Europe%2FBerlin";
    let (status, body) = get_raw(router, uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["warning"].is_null());
}
