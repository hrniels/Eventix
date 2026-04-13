// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use tempfile::TempDir;

use crate::helper::{CAL_ID, encode_form, get, make_router, make_state, post};

use super::write_event_ics;

// --- GET /api/items/editalarm ---

/// Fetching the alarm editor for an existing event returns HTTP 200 with a JSON body whose `html`
/// key contains the rendered alarm editor widget.
#[tokio::test]
async fn get_returns_html() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "editalarm-event";
    write_event_ics(&cal_dir, uid, "Meeting with alarm");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = format!("/api/items/editalarm?uid={uid}&edit=false");
    let (status, body) = get(router, &uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("response is not valid JSON: {body}"));
    assert!(
        json.get("html").and_then(|v| v.as_str()).is_some(),
        "expected 'html' key in response JSON, got: {json}"
    );
}

/// Fetching the alarm editor for an unknown UID returns a non-200 status.
#[tokio::test]
async fn get_unknown_uid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_ics(&cal_dir, "other", "Other event");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let (status, _) = get(router, "/api/items/editalarm?uid=no-such-uid&edit=false").await;
    assert_ne!(status.as_u16(), 200);
}

// --- POST /api/items/editalarm ---

/// Posting an alarm configuration with `personal_overwrite` set saves a personal alarm override.
/// The endpoint returns HTTP 200 with an empty JSON body.
#[tokio::test]
async fn post_saves_alarm() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "editalarm-save";
    write_event_ics(&cal_dir, uid, "Meeting");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // personal[trigger]=RELATIVE, personal[duration]=15, personal[durunit]=Minutes,
    // personal[durtype]=BeforeStart, personal_overwrite=1 (set)
    let body = encode_form(&[
        ("uid", uid),
        ("personal[trigger]", "RELATIVE"),
        ("personal[duration]", "15"),
        ("personal[durunit]", "Minutes"),
        ("personal[durtype]", "BeforeStart"),
        ("personal_overwrite", "1"),
    ]);
    let (status, resp_body) = post(router, "/api/items/editalarm", &body).await;
    assert_eq!(status, 200, "response body: {resp_body}");
}

/// Posting without `personal_overwrite` clears any existing personal alarm override.
#[tokio::test]
async fn post_clears_alarm() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "editalarm-clear";
    write_event_ics(&cal_dir, uid, "Meeting");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Omit personal_overwrite to request a clear.
    let body = encode_form(&[("uid", uid)]);
    let (status, _) = post(router, "/api/items/editalarm", &body).await;
    assert_eq!(status, 200);
}
