// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../helper/mod.rs"]
mod helper;

use axum::http::StatusCode;
use eventix_state::{CalendarSettings, CollectionSettings, Settings, SyncerType, load_from_file};
use serde_json::Value;
use tempfile::TempDir;

use helper::{CAL_ID, COL_ID, get, make_calendars_api_router, make_state_from_col, post_query};

fn make_collection(tmp: &TempDir) -> CollectionSettings {
    let calendars_path = tmp.path().join("calendars");
    let cal_dir = calendars_path.join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).expect("create calendar dir");

    let mut collection = CollectionSettings::new(SyncerType::FileSystem {
        path: calendars_path.to_string_lossy().into_owned(),
    });
    let mut calendar = CalendarSettings::default();
    calendar.set_enabled(true);
    calendar.set_folder(CAL_ID.to_string());
    calendar.set_name("Test Calendar".to_string());
    collection
        .all_calendars_mut()
        .insert(CAL_ID.to_string(), calendar);
    collection
}

// --- POST /api/collections/delete ---

#[tokio::test]
async fn delete_collection_removes_settings_store_and_log() {
    let source_tmp = TempDir::new().unwrap();
    let (state, xdg_tmp) = make_state_from_col(make_collection(&source_tmp));

    let log_path = {
        let locked = state.lock().await;
        eventix_state::log_file(locked.xdg(), &COL_ID.to_string())
    };
    std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
    std::fs::write(&log_path, "sync log\n").unwrap();

    let router = make_calendars_api_router(state.clone());
    let (status, body) =
        post_query(router, &format!("/api/collections/delete?col_id={COL_ID}")).await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");
    assert_eq!(body, "null");

    let locked = state.lock().await;
    assert!(!locked.settings().collections().contains_key(COL_ID));
    assert!(locked.store().directories().is_empty());
    drop(locked);

    assert!(!log_path.exists(), "expected sync log to be removed");

    let settings: Settings = load_from_file(&xdg_tmp.path().join("settings.toml")).unwrap();
    assert!(!settings.collections().contains_key(COL_ID));
}

#[tokio::test]
async fn delete_unknown_collection_returns_error() {
    let source_tmp = TempDir::new().unwrap();
    let (state, _xdg_tmp) = make_state_from_col(make_collection(&source_tmp));

    let router = make_calendars_api_router(state);
    let (status, body) = post_query(router, "/api/collections/delete?col_id=unknown").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("Unable to delete collection unknown"));
}

// --- GET /api/collections/log ---

#[tokio::test]
async fn log_returns_no_entries_when_log_file_is_missing() {
    let source_tmp = TempDir::new().unwrap();
    let (state, _xdg_tmp) = make_state_from_col(make_collection(&source_tmp));

    let router = make_calendars_api_router(state);
    let (status, body) = get(router, &format!("/api/collections/log?col_id={COL_ID}")).await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");

    let html = body_to_html(&body);
    assert!(html.contains("Log of collection"));
    assert!(html.contains(COL_ID));
    assert!(html.contains("- No entries -"));
}

#[tokio::test]
async fn log_returns_rendered_log_contents() {
    let source_tmp = TempDir::new().unwrap();
    let (state, _xdg_tmp) = make_state_from_col(make_collection(&source_tmp));

    let log_path = {
        let locked = state.lock().await;
        eventix_state::log_file(locked.xdg(), &COL_ID.to_string())
    };
    std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
    std::fs::write(&log_path, "line one\nline two\n").unwrap();

    let router = make_calendars_api_router(state);
    let (status, body) = get(router, &format!("/api/collections/log?col_id={COL_ID}")).await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");

    let html = body_to_html(&body);
    assert!(html.contains("Log of collection"));
    assert!(html.contains(COL_ID));
    assert!(html.contains("from"));
    assert!(html.contains("line one"));
    assert!(html.contains("line two"));
    assert!(html.contains("<pre id=\"log\">"));
}

fn body_to_html(body: &str) -> String {
    let json: Value = serde_json::from_str(body).expect("parse JSON response");
    json.get("html")
        .and_then(Value::as_str)
        .expect("html field")
        .to_string()
}
