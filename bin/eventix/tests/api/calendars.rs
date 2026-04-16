// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../helper/mod.rs"]
mod helper;

use axum::http::StatusCode;
use eventix_state::{CollectionSettings, SyncerType};
use tempfile::TempDir;

use helper::{COL_ID, make_calendars_api_router, make_state_from_col, post_query};

fn make_filesystem_collection(tmp: &TempDir) -> CollectionSettings {
    let calendars_path = tmp.path().join("calendars");
    std::fs::create_dir_all(&calendars_path).expect("create calendars dir");

    CollectionSettings::new(SyncerType::FileSystem {
        path: calendars_path.to_string_lossy().into_owned(),
    })
}

#[tokio::test]
async fn add_calendar_creates_calendar() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_filesystem_collection(&tmp));

    let router = make_calendars_api_router(state.clone());
    let (status, resp) = post_query(
        router,
        "/api/calendars/addcal?col_id=col1&name=Local%20Team",
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{resp}");

    let locked = state.lock().await;
    let calendars = locked
        .settings()
        .collections()
        .get(COL_ID)
        .unwrap()
        .all_calendars();
    let added = calendars
        .values()
        .next()
        .expect("expected inserted calendar");
    assert_eq!(added.name(), "Local Team");
    assert_eq!(added.folder(), "local-team");
    assert!(added.enabled());
    assert!(tmp.path().join("calendars/local-team").exists());
}

#[tokio::test]
async fn add_calendar_sanitizes_and_deduplicates_folder_name() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_filesystem_collection(&tmp));

    let router1 = make_calendars_api_router(state.clone());
    let (status1, resp1) = post_query(
        router1,
        "/api/calendars/addcal?col_id=col1&name=Team%20%26%20Ops",
    )
    .await;
    assert_eq!(status1, StatusCode::OK, "unexpected body:\n{resp1}");

    let router2 = make_calendars_api_router(state.clone());
    let (status2, resp2) =
        post_query(router2, "/api/calendars/addcal?col_id=col1&name=Team%20Ops").await;
    assert_eq!(status2, StatusCode::OK, "unexpected body:\n{resp2}");

    let locked = state.lock().await;
    let mut folders = locked
        .settings()
        .collections()
        .get(COL_ID)
        .unwrap()
        .all_calendars()
        .values()
        .map(|cal| cal.folder().clone())
        .collect::<Vec<_>>();
    folders.sort();

    assert_eq!(
        folders,
        vec!["team-ops".to_string(), "team-ops-2".to_string()]
    );
    assert!(tmp.path().join("calendars/team-ops").exists());
    assert!(tmp.path().join("calendars/team-ops-2").exists());
}

#[tokio::test]
async fn add_calendar_rejects_empty_name() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_filesystem_collection(&tmp));

    let router = make_calendars_api_router(state);
    let (status, resp) = post_query(router, "/api/calendars/addcal?col_id=col1&name=%20%20").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        resp.contains("Please enter a calendar name!"),
        "expected validation error, got:\n{resp}"
    );
}
