// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use tempfile::TempDir;

use crate::helper::{CAL_ID, get, make_router, make_state};

use super::write_event_ics;

// --- GET /api/items/tzconvert ---

/// Converting a single from-date/time pair to a different timezone returns the converted values.
#[tokio::test]
async fn convert_from_date() {
    // Europe/Berlin is UTC+1 in April (before DST ends); converting 10:00 to UTC gives 09:00.
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "dummy", "Dummy");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = "/api/items/tzconvert\
               ?from_date=2026-04-15&from_time=10:00\
               &from_tz=Europe%2FBerlin&to_tz=UTC";
    let (status, body) = get(router, uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["from_date"], "2026-04-15");
    assert_eq!(json["from_time"], "08:00");
}

/// Converting a to-date/time pair (while from fields are absent) converts that pair.
#[tokio::test]
async fn convert_to_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "dummy", "Dummy");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = "/api/items/tzconvert\
               ?to_date=2026-04-15&to_time=10:00\
               &from_tz=Europe%2FBerlin&to_tz=UTC";
    let (status, body) = get(router, uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["to_date"], "2026-04-15");
    assert_eq!(json["to_time"], "08:00");
}

/// Providing both from and to date/time pairs converts both independently.
#[tokio::test]
async fn convert_both() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "dummy", "Dummy");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = "/api/items/tzconvert\
               ?from_date=2026-04-15&from_time=09:00\
               &to_date=2026-04-15&to_time=10:00\
               &from_tz=Europe%2FBerlin&to_tz=UTC";
    let (status, body) = get(router, uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["from_time"], "07:00");
    assert_eq!(json["to_time"], "08:00");
}

/// Empty from/to fields are passed through unchanged.
#[tokio::test]
async fn empty_fields_pass_through() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "dummy", "Dummy");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = "/api/items/tzconvert\
               ?from_date=&from_time=&to_date=&to_time=\
               &from_tz=Europe%2FBerlin&to_tz=UTC";
    let (status, body) = get(router, uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    // All fields should remain empty (or null).
    let from_date = json["from_date"].as_str().unwrap_or("");
    let to_date = json["to_date"].as_str().unwrap_or("");
    assert_eq!(from_date, "");
    assert_eq!(to_date, "");
}

/// Supplying an unknown timezone name returns a non-200 status.
#[tokio::test]
async fn invalid_timezone_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "dummy", "Dummy");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = "/api/items/tzconvert\
               ?from_date=2026-04-15&from_time=10:00\
               &from_tz=Not%2FA%2FTimezone&to_tz=UTC";
    let (status, _) = get(router, uri).await;
    assert_ne!(status.as_u16(), 200);
}

/// Supplying a malformed date string returns a non-200 status.
#[tokio::test]
async fn invalid_date_format_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "dummy", "Dummy");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = "/api/items/tzconvert\
               ?from_date=not-a-date&from_time=10:00\
               &from_tz=Europe%2FBerlin&to_tz=UTC";
    let (status, _) = get(router, uri).await;
    assert_ne!(status.as_u16(), 200);
}
