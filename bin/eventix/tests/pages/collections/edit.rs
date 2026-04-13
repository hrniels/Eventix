// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../../helper/mod.rs"]
mod helper;

use axum::http::StatusCode;
use eventix_state::{CollectionSettings, SyncerType};
use tempfile::TempDir;

use helper::collections::assert_success;
use helper::{
    COL_ID, assert_error, encode_form, make_collections_router, make_state_from_col, merge_fields,
};

fn fs_fields(fs_path: &str) -> Vec<(&'static str, &str)> {
    vec![
        ("syncer[syncer]", "FILESYSTEM"),
        ("syncer[fs_path]", fs_path),
        ("syncer[vdir_name]", ""),
        ("syncer[vdir_email]", ""),
        ("syncer[vdir_url]", ""),
        ("syncer[vdir_username]", ""),
        ("syncer[vdir_pw_cmd]", ""),
        ("syncer[vdir_time_span]", "infinite"),
        ("syncer[vdir_time_span_years]", "5"),
        ("syncer[o365_name]", ""),
        ("syncer[o365_email]", ""),
        ("syncer[o365_pw_cmd]", ""),
        ("syncer[o365_time_span]", "infinite"),
        ("syncer[o365_time_span_years]", "5"),
    ]
}

fn vdir_fields<'a>(name: &'a str, email: &'a str, url: &'a str) -> Vec<(&'static str, &'a str)> {
    vec![
        ("syncer[syncer]", "VDIRSYNCER"),
        ("syncer[vdir_name]", name),
        ("syncer[vdir_email]", email),
        ("syncer[vdir_url]", url),
        ("syncer[vdir_username]", ""),
        ("syncer[vdir_pw_cmd]", ""),
        ("syncer[vdir_time_span]", "infinite"),
        ("syncer[vdir_time_span_years]", "5"),
        ("syncer[fs_path]", ""),
        ("syncer[o365_name]", ""),
        ("syncer[o365_email]", ""),
        ("syncer[o365_pw_cmd]", ""),
        ("syncer[o365_time_span]", "infinite"),
        ("syncer[o365_time_span_years]", "5"),
    ]
}

/// Builds an `EventixState` seeded with a `FileSystem` collection pointing at `fs_path`.
///
/// Returns both the state and the config `TempDir`, which must be kept alive for the duration of
/// the test whenever the handler may write settings to disk.
fn make_fs_state(fs_path: &str) -> (eventix_state::EventixState, TempDir) {
    let col = CollectionSettings::new(SyncerType::FileSystem {
        path: fs_path.to_string(),
    });
    make_state_from_col(col)
}

/// Builds an `EventixState` seeded with a `VDirSyncer` collection.
///
/// Returns both the state and the config `TempDir`, which must be kept alive for the duration of
/// the test whenever the handler may write settings to disk.
fn make_vdir_state() -> (eventix_state::EventixState, TempDir) {
    let col = CollectionSettings::new(SyncerType::VDirSyncer {
        email: eventix_state::EmailAccount::new(
            "Alice".to_string(),
            "alice@example.com".to_string(),
        ),
        url: "https://dav.example.com/".to_string(),
        read_only: false,
        username: None,
        password_cmd: None,
        time_span: eventix_state::SyncTimeSpan {
            start: eventix_state::SyncTimeBound::Infinite,
            end: eventix_state::SyncTimeBound::Infinite,
        },
    });
    make_state_from_col(col)
}

#[tokio::test]
async fn edit_filesystem_success() {
    let tmp = TempDir::new().unwrap();
    let orig_path = tmp.path().to_string_lossy().into_owned();
    let (state, _cfg) = make_fs_state(&orig_path);

    // Update the path to the same directory — it is a real existing directory.
    let new_path = orig_path.clone();
    let fields = fs_fields(&new_path);
    let body = encode_form(&fields);

    let router = make_collections_router(state.clone());
    let uri = format!("/collections/edit?col_id={COL_ID}");
    let (status, resp) = helper::post(router, &uri, &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_success(&resp);

    // Verify the updated path was persisted in the settings.
    let locked = state.lock().await;
    let col = locked.settings().collections().get(COL_ID).unwrap();
    assert!(
        matches!(col.syncer(), SyncerType::FileSystem { path } if path == &new_path),
        "expected updated fs_path in settings"
    );
}

#[tokio::test]
async fn edit_filesystem_empty_path() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_fs_state(&tmp.path().to_string_lossy());

    let fields = fs_fields("");
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let uri = format!("/collections/edit?col_id={COL_ID}");
    let (status, resp) = helper::post(router, &uri, &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify the path"),
        "expected collection_path error, got:\n{resp}"
    );
}

#[tokio::test]
async fn edit_filesystem_nonexistent_path() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_fs_state(&tmp.path().to_string_lossy());

    let fields = fs_fields("/this/path/does/not/exist");
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let uri = format!("/collections/edit?col_id={COL_ID}");
    let (status, resp) = helper::post(router, &uri, &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify an existing directory"),
        "expected collection_existing_dir error, got:\n{resp}"
    );
}

#[tokio::test]
async fn edit_syncer_type_change() {
    let tmp = TempDir::new().unwrap();
    // Seed with a FileSystem collection, then attempt to switch to VDirSyncer.
    let (state, _cfg) = make_fs_state(&tmp.path().to_string_lossy());

    let fields = vdir_fields("Alice", "alice@example.com", "https://dav.example.com/");
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let uri = format!("/collections/edit?col_id={COL_ID}");
    let (status, resp) = helper::post(router, &uri, &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("You cannot change the syncer type"),
        "expected syncer_change error, got:\n{resp}"
    );
}

#[tokio::test]
async fn edit_vdirsync_success() {
    let (state, _cfg) = make_vdir_state();

    let fields = vdir_fields("Bob", "bob@example.com", "https://newdav.example.com/");
    let body = encode_form(&fields);

    let router = make_collections_router(state.clone());
    let uri = format!("/collections/edit?col_id={COL_ID}");
    let (status, resp) = helper::post(router, &uri, &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_success(&resp);

    // Verify the updated syncer settings were persisted.
    let locked = state.lock().await;
    let col = locked.settings().collections().get(COL_ID).unwrap();
    assert!(
        matches!(col.syncer(), SyncerType::VDirSyncer { url, .. } if url == "https://newdav.example.com/"),
        "expected updated vdir URL in settings"
    );
}

#[tokio::test]
async fn edit_vdirsync_invalid_email() {
    let (state, _cfg) = make_vdir_state();

    let fields = vdir_fields("Alice", "not-an-email", "https://dav.example.com/");
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let uri = format!("/collections/edit?col_id={COL_ID}");
    let (status, resp) = helper::post(router, &uri, &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify a valid email address"),
        "expected collection_your_email error, got:\n{resp}"
    );
}

#[tokio::test]
async fn edit_vdirsync_time_span_years_too_large() {
    let (state, _cfg) = make_vdir_state();

    let base = vdir_fields("Alice", "alice@example.com", "https://dav.example.com/");
    let fields = merge_fields(
        base,
        &[
            ("syncer[vdir_time_span]", "years"),
            ("syncer[vdir_time_span_years]", "101"),
        ],
    );
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let uri = format!("/collections/edit?col_id={COL_ID}");
    let (status, resp) = helper::post(router, &uri, &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify at most 100 years"),
        "expected collection_time_span_years error, got:\n{resp}"
    );
}

#[tokio::test]
async fn edit_unknown_col_id() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_fs_state(&tmp.path().to_string_lossy());

    let new_path = tmp.path().to_string_lossy().into_owned();
    let fields = fs_fields(&new_path);
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    // Use a col_id that does not exist in the settings.
    let (status, _resp) = helper::post(router, "/collections/edit?col_id=nonexistent", &body).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}
