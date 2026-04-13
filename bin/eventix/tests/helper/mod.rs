// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[allow(dead_code)]
pub mod collections;
#[allow(dead_code)]
pub mod create;
#[allow(dead_code)]
pub mod edit;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use eventix_ical::col::CalFile;
use eventix_state::{
    CalendarSettings, CollectionSettings, EmailAccount, EventixState, Settings, SyncerType,
};
use tempfile::TempDir;
use tower::ServiceExt;

/// Calendar and collection IDs used across all tests.
pub const COL_ID: &str = "col1";
pub const CAL_ID: &str = "cal1";
#[allow(dead_code)]
pub const CAL2_ID: &str = "cal2";

/// Creates an `EventixState` backed by a temporary `FileSystem` calendar directory.
///
/// Both the config directory and the data directory (used for locale files and personal alarms)
/// are placed under the parent of `cal_dir` so that they remain alive as long as the caller's
/// `TempDir` is alive. This is required for endpoints that write to the personal alarms store.
///
/// The calendar directory `cal_dir` is used directly as the folder for the test calendar entry.
/// The parent of `cal_dir` is set as both the `FileSystem` collection path and the XDG data home,
/// so that `State::new` finds locale files and the alarms directory at that location.
///
/// Returns the state. The caller must keep the `TempDir` alive for the duration of the test.
#[allow(dead_code)]
pub fn make_state(cal_dir: &Path) -> EventixState {
    let col_path = cal_dir.parent().unwrap();
    let mut col = CollectionSettings::new(SyncerType::FileSystem {
        path: col_path.to_string_lossy().into_owned(),
    });

    let mut cal = CalendarSettings::default();
    cal.set_enabled(true);
    // folder is the last component of cal_dir (e.g. "cal1"), which State::new joins onto the
    // collection path to produce the full calendar directory.
    cal.set_folder(cal_dir.file_name().unwrap().to_string_lossy().into_owned());
    cal.set_name("Test Calendar".to_string());
    col.all_calendars_mut().insert(CAL_ID.to_string(), cal);

    // Use col_path as the XDG data home so that locale files and the alarms directory persist
    // for the duration of the test (the caller's TempDir owns col_path).
    make_state_from_col_in(col, col_path)
}

/// Creates an `EventixState` backed by two calendar directories under the same collection.
///
/// `cal_dir` is the directory for `CAL_ID` (the source calendar); a sibling directory named
/// `CAL2_ID` is created next to it for the second calendar. Both are registered under `COL_ID`.
///
/// Returns the state and the path to the second calendar directory.
/// The caller must keep the `TempDir` that owns both directories alive for the duration of the
/// test.
#[allow(dead_code)]
pub fn make_state_two_cals(cal_dir: &Path) -> (EventixState, std::path::PathBuf) {
    let col_path = cal_dir.parent().unwrap();
    let mut col = CollectionSettings::new(SyncerType::FileSystem {
        path: col_path.to_string_lossy().into_owned(),
    });

    let mut cal1 = CalendarSettings::default();
    cal1.set_enabled(true);
    cal1.set_folder(cal_dir.file_name().unwrap().to_string_lossy().into_owned());
    cal1.set_name("Test Calendar".to_string());
    col.all_calendars_mut().insert(CAL_ID.to_string(), cal1);

    let cal2_dir = col_path.join(CAL2_ID);
    std::fs::create_dir_all(&cal2_dir).unwrap();
    let mut cal2 = CalendarSettings::default();
    cal2.set_enabled(true);
    cal2.set_folder(CAL2_ID.to_string());
    cal2.set_name("Other Calendar".to_string());
    col.all_calendars_mut().insert(CAL2_ID.to_string(), cal2);

    (make_state_from_col_in(col, col_path), cal2_dir)
}

/// Creates an `EventixState` backed by a VDirSyncer collection that carries a test email account.
///
/// Returns the state and the path to the calendar directory (`cal_dir`). The caller must keep the
/// provided `TempDir` alive for the duration of the test.
///
/// The email account is `test@example.com`. Using a VDirSyncer syncer is necessary because
/// `FileSystem` syncers never carry an email; endpoints that require `util::user_for_uid` to
/// return a non-`None` account (e.g. `respond`) need this variant.
///
/// The directory layout matches VDirSyncer's path resolution:
/// `{xdg_data_home}/vdirsyncer/{COL_ID}-data/{CAL_ID}`.
#[allow(dead_code)]
pub fn make_state_with_email(tmp: &TempDir) -> (EventixState, PathBuf) {
    // VDirSyncer resolves its collection path as:
    //   xdg.get_data_file("vdirsyncer").join("{col_id}-data")
    // i.e. {xdg_data_home}/vdirsyncer/{col_id}-data/{cal_folder}
    let data_home = tmp.path().to_path_buf();
    let cal_dir = data_home
        .join("vdirsyncer")
        .join(format!("{COL_ID}-data"))
        .join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let syncer = SyncerType::VDirSyncer {
        email: EmailAccount::new("Test User".to_string(), "test@example.com".to_string()),
        url: "http://localhost".to_string(),
        read_only: false,
        username: None,
        password_cmd: None,
        time_span: Default::default(),
    };
    let mut col = CollectionSettings::new(syncer);
    let mut cal = CalendarSettings::default();
    cal.set_enabled(true);
    cal.set_folder(CAL_ID.to_string());
    cal.set_name("Test Calendar".to_string());
    col.all_calendars_mut().insert(CAL_ID.to_string(), cal);

    let config_tmp = TempDir::new().unwrap();
    let locale_dir = data_home.join("locale");
    std::fs::create_dir_all(&locale_dir).unwrap();
    std::fs::write(
        locale_dir.join("English.toml"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/locale/English.toml"
        )),
    )
    .unwrap();

    let xdg = Arc::new(eventix_state::with_test_xdg(&data_home, config_tmp.path()));
    let mut settings = Settings::new(xdg.get_config_home().unwrap().join("settings.toml"));
    settings.collections_mut().insert(COL_ID.to_string(), col);
    settings.write_to_file().expect("write settings");

    let state = eventix_state::State::new(xdg).expect("State::new");
    (Arc::new(tokio::sync::Mutex::new(state)), cal_dir)
}

/// Constructs an `EventixState` from the given collection settings, placing XDG data under
/// `data_home` (which must remain alive for the duration of the test).
///
/// Using a caller-owned directory for the data home allows the personal alarms store to write
/// files back to disk during tests that exercise save paths.
fn make_state_from_col_in(col: CollectionSettings, data_home: &Path) -> EventixState {
    let config_tmp = TempDir::new().unwrap();

    let locale_dir = data_home.join("locale");
    std::fs::create_dir_all(&locale_dir).unwrap();
    std::fs::write(
        locale_dir.join("English.toml"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/locale/English.toml"
        )),
    )
    .unwrap();

    let xdg = Arc::new(eventix_state::with_test_xdg(data_home, config_tmp.path()));

    let mut settings = Settings::new(xdg.get_config_home().unwrap().join("settings.toml"));
    settings.collections_mut().insert(COL_ID.to_string(), col);
    settings.write_to_file().expect("write settings");

    let state = eventix_state::State::new(xdg).expect("State::new");
    Arc::new(tokio::sync::Mutex::new(state))
}

/// Constructs an `EventixState` from the given collection settings.
///
/// Returns the state together with the config `TempDir`. The caller must keep the `TempDir` alive
/// for as long as the state may write settings back to disk (e.g. in collections add/edit tests).
/// Tests that never write settings may discard the returned `TempDir` immediately.
#[allow(dead_code)]
pub fn make_state_from_col(col: CollectionSettings) -> (EventixState, TempDir) {
    let config_tmp = TempDir::new().unwrap();
    let data_tmp = TempDir::new().unwrap();

    // Write the locale files into the temp data directory so that State::new can find them via
    // XDG_DATA_HOME without reading from the project source tree.
    let locale_dir = data_tmp.path().join("locale");
    std::fs::create_dir_all(&locale_dir).unwrap();
    std::fs::write(
        locale_dir.join("English.toml"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/locale/English.toml"
        )),
    )
    .unwrap();

    let xdg = Arc::new(eventix_state::with_test_xdg(
        data_tmp.path(),
        config_tmp.path(),
    ));

    let mut settings = Settings::new(xdg.get_config_home().unwrap().join("settings.toml"));
    settings.collections_mut().insert(COL_ID.to_string(), col);
    settings.write_to_file().expect("write settings");

    let state = eventix_state::State::new(xdg).expect("State::new");
    (Arc::new(tokio::sync::Mutex::new(state)), config_tmp)
}

/// Builds a minimal axum `Router` wiring only the add-item endpoints.
///
/// This is sufficient for create-event and create-todo tests. The pages router is mounted at
/// `/pages/items/add` and the API items router at `/api/items`.
#[allow(dead_code)]
pub fn make_router(state: EventixState) -> Router {
    Router::new()
        .nest("/pages/items", eventix::pages::items::router(state.clone()))
        .nest("/api/items", eventix::api::items::router(state))
}

/// Builds an axum `Router` wiring only the collections page endpoints.
///
/// Routes are mounted at `/collections`, matching the path used by the real application.
#[allow(dead_code)]
pub fn make_collections_router(state: EventixState) -> Router {
    Router::new().nest("/collections", eventix::pages::collections::router(state))
}

/// Sends a POST to `uri` with the given `application/x-www-form-urlencoded` body and returns the
/// status code and response body text.
pub async fn post(router: Router, uri: &str, body: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body.to_owned()))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8_lossy(&bytes).into_owned();
    (status, body)
}

/// Sends a POST to `uri` (which may include query parameters) with an empty body and returns the
/// status code and response body text.
///
/// Use this for endpoints that read parameters from the query string (`Query<T>` extractor) rather
/// than the request body.
#[allow(dead_code)]
pub async fn post_query(router: Router, uri: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8_lossy(&bytes).into_owned();
    (status, body)
}

/// Sends a GET to `uri` and returns the status code and response body text.
#[allow(dead_code)]
pub async fn get(router: Router, uri: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8_lossy(&bytes).into_owned();
    (status, body)
}

/// Asserts that no `.ics` file was created in `cal_dir`.
#[allow(unused)]
pub fn assert_no_ics(cal_dir: &Path) {
    let count = std::fs::read_dir(cal_dir)
        .unwrap()
        .filter(|e| {
            let p = e.as_ref().unwrap().path();
            p.extension().and_then(|s| s.to_str()) == Some("ics")
        })
        .count();
    assert_eq!(count, 0, "expected no .ics files but found {count}");
}

/// Asserts that the HTML response body contains an error banner and no success info banner.
#[allow(dead_code)]
pub fn assert_error(body: &str) {
    assert!(
        body.contains("ev_msg_error"),
        "expected error banner in response, got:\n{body}"
    );
}

/// Returns the first component from `cal_file`.
///
/// Panics if the file contains no components.
#[allow(dead_code)]
pub fn first_component(cal_file: &CalFile) -> &eventix_ical::objects::CalComponent {
    let comps = cal_file.components();
    assert!(
        !comps.is_empty(),
        "expected at least one component in the ICS file"
    );
    &comps[0]
}

/// Merges two slices of form fields, with entries in `overrides` replacing entries in `base` that
/// share the same key. Entries in `overrides` not present in `base` are appended.
#[allow(dead_code)]
pub fn merge_fields<'a>(
    base: Vec<(&'a str, &'a str)>,
    overrides: &[(&'a str, &'a str)],
) -> Vec<(&'a str, &'a str)> {
    let mut result: Vec<(&str, &str)> = base;
    for &(k, v) in overrides {
        if let Some(pos) = result.iter().position(|(bk, _)| *bk == k) {
            result[pos] = (k, v);
        } else {
            result.push((k, v));
        }
    }
    result
}

/// Percent-encodes all non-alphanumeric characters except `-`, `_`, `.`, and `~` as per RFC 3986.
pub fn encode_form(fields: &[(&str, &str)]) -> String {
    fn encode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char);
                }
                _ => {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
        out
    }

    fields
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}
