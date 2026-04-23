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

// --- State setup ---

/// Creates an `EventixState` with a single empty `FileSystem` collection and a writable config
/// directory. The config `TempDir` must be kept alive for the duration of the test.
fn make_add_state(fs_path: &str) -> (eventix_state::EventixState, TempDir) {
    let col = CollectionSettings::new(SyncerType::FileSystem {
        path: fs_path.to_string(),
    });
    make_state_from_col(col)
}

fn base_fs_fields<'a>(name: &'a str, fs_path: &'a str) -> Vec<(&'static str, &'a str)> {
    vec![
        ("name", name),
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

fn base_vdir_fields(name: &str) -> Vec<(&'static str, &str)> {
    vec![
        ("name", name),
        ("syncer[syncer]", "VDIRSYNCER"),
        ("syncer[vdir_name]", "Alice"),
        ("syncer[vdir_email]", "alice@example.com"),
        ("syncer[vdir_url]", "https://dav.example.com/"),
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

fn base_o365_fields(name: &str) -> Vec<(&'static str, &str)> {
    vec![
        ("name", name),
        ("syncer[syncer]", "O365"),
        ("syncer[o365_name]", "Bob"),
        ("syncer[o365_email]", "bob@example.com"),
        // O365 requires a non-empty password command (unwrap in to_syncer panics otherwise).
        ("syncer[o365_pw_cmd]", "pass show myaccount"),
        ("syncer[o365_time_span]", "infinite"),
        ("syncer[o365_time_span_years]", "5"),
        ("syncer[fs_path]", ""),
        ("syncer[vdir_name]", ""),
        ("syncer[vdir_email]", ""),
        ("syncer[vdir_url]", ""),
        ("syncer[vdir_username]", ""),
        ("syncer[vdir_pw_cmd]", ""),
        ("syncer[vdir_time_span]", "infinite"),
        ("syncer[vdir_time_span_years]", "5"),
    ]
}

#[tokio::test]
async fn add_filesystem_success() {
    let tmp = TempDir::new().unwrap();
    let fs_path = tmp.path().to_string_lossy().into_owned();
    let (state, _cfg) = make_add_state(&fs_path);

    let fields = base_fs_fields("mynewcol", &fs_path);
    let body = encode_form(&fields);

    let router = make_collections_router(state.clone());
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_success(&resp);

    // Verify the collection was persisted in the state settings.
    let locked = state.lock().await;
    assert!(
        locked.settings().collections().contains_key("mynewcol"),
        "expected collection 'mynewcol' in settings after add"
    );
}

#[tokio::test]
async fn add_vdirsync_success() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = base_vdir_fields("vdircol");
    let body = encode_form(&fields);

    let router = make_collections_router(state.clone());
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_success(&resp);

    let locked = state.lock().await;
    assert!(
        locked.settings().collections().contains_key("vdircol"),
        "expected collection 'vdircol' in settings after add"
    );
}

#[tokio::test]
async fn add_o365_success() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = base_o365_fields("o365col");
    let body = encode_form(&fields);

    let router = make_collections_router(state.clone());
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_success(&resp);

    let locked = state.lock().await;
    assert!(
        locked.settings().collections().contains_key("o365col"),
        "expected collection 'o365col' in settings after add"
    );
}

#[tokio::test]
async fn add_empty_name() {
    let tmp = TempDir::new().unwrap();
    let fs_path = tmp.path().to_string_lossy().into_owned();
    let (state, _cfg) = make_add_state(&fs_path);

    let fields = merge_fields(base_fs_fields("", &fs_path), &[("name", "")]);
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("non-empty name that only contains"),
        "expected name_chars error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_invalid_name_chars() {
    let tmp = TempDir::new().unwrap();
    let fs_path = tmp.path().to_string_lossy().into_owned();
    let (state, _cfg) = make_add_state(&fs_path);

    // Name contains a space — not allowed.
    let fields = base_fs_fields("invalid name", &fs_path);
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("non-empty name that only contains"),
        "expected name_chars error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_duplicate_name() {
    let tmp = TempDir::new().unwrap();
    let fs_path = tmp.path().to_string_lossy().into_owned();
    let (state, _cfg) = make_add_state(&fs_path);

    // COL_ID is already in the state's settings, so adding it again should fail.
    let fields = base_fs_fields(COL_ID, &fs_path);
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("A collection with that name does already exist"),
        "expected collection_exists error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_filesystem_empty_path() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = base_fs_fields("newcol", "");
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify the path"),
        "expected collection_path error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_filesystem_nonexistent_path() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = base_fs_fields("newcol", "/this/path/does/not/exist");
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify an existing directory"),
        "expected collection_existing_dir error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_vdirsync_empty_name() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = merge_fields(base_vdir_fields("vdircol"), &[("syncer[vdir_name]", "")]);
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify your name"),
        "expected collection_your_name error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_vdirsync_invalid_email() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = merge_fields(
        base_vdir_fields("vdircol"),
        &[("syncer[vdir_email]", "not-an-email")],
    );
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify a valid email address"),
        "expected collection_your_email error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_vdirsync_invalid_url() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = merge_fields(
        base_vdir_fields("vdircol"),
        &[("syncer[vdir_url]", "not a url at all")],
    );
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify a valid location"),
        "expected collection_location error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_vdirsync_time_span_years_too_large() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = merge_fields(
        base_vdir_fields("vdircol"),
        &[
            ("syncer[vdir_time_span]", "years"),
            ("syncer[vdir_time_span_years]", "101"),
        ],
    );
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify at most 100 years"),
        "expected collection_time_span_years error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_o365_empty_name() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = merge_fields(base_o365_fields("o365col"), &[("syncer[o365_name]", "")]);
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify your name"),
        "expected collection_your_name error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_o365_invalid_email() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = merge_fields(
        base_o365_fields("o365col"),
        &[("syncer[o365_email]", "not-an-email")],
    );
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify a valid email address"),
        "expected collection_your_email error, got:\n{resp}"
    );
}

#[tokio::test]
async fn add_o365_time_span_years_too_large() {
    let tmp = TempDir::new().unwrap();
    let (state, _cfg) = make_add_state(&tmp.path().to_string_lossy());

    let fields = merge_fields(
        base_o365_fields("o365col"),
        &[
            ("syncer[o365_time_span]", "years"),
            ("syncer[o365_time_span_years]", "101"),
        ],
    );
    let body = encode_form(&fields);

    let router = make_collections_router(state);
    let (status, resp) = helper::post(router, "/collections/add", &body).await;

    assert_eq!(status, StatusCode::OK);
    assert_error(&resp);
    assert!(
        resp.contains("Please specify at most 100 years"),
        "expected collection_time_span_years error, got:\n{resp}"
    );
}
