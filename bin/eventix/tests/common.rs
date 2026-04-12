// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use eventix_ical::col::CalFile;
use eventix_state::{CalendarSettings, CollectionSettings, EventixState, Settings, SyncerType};
use tempfile::TempDir;
use tower::ServiceExt;

/// Calendar and collection IDs used across all tests.
pub const COL_ID: &str = "col1";
pub const CAL_ID: &str = "cal1";

/// Creates an `EventixState` backed by a temporary `FileSystem` calendar directory.
///
/// Both the config directory and the data directory (used for locale files) are placed in
/// temporary directories so that tests are fully self-contained and do not read from the project
/// source tree.
///
/// The calendar directory `cal_dir` is used directly as the folder for the test calendar entry.
/// The parent of `cal_dir` is set as the `FileSystem` collection path so that the state constructs
/// the full calendar path as `parent(cal_dir) / CAL_ID`, matching `cal_dir` exactly.
///
/// Returns the state. The caller must keep the `TempDir` alive for the duration of the test.
pub fn make_state(cal_dir: &Path) -> EventixState {
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

    let mut cal = CalendarSettings::default();
    cal.set_enabled(true);
    // folder is the last component of cal_dir (e.g. "cal1"), which State::new joins onto the
    // collection path to produce the full calendar directory.
    cal.set_folder(cal_dir.file_name().unwrap().to_string_lossy().into_owned());
    cal.set_name("Test Calendar".to_string());

    // The FileSystem collection path is the *parent* of cal_dir so that
    //   col_path.join(cal.folder()) == cal_dir
    let col_path = cal_dir.parent().unwrap();
    let mut col = CollectionSettings::new(SyncerType::FileSystem {
        path: col_path.to_string_lossy().into_owned(),
    });
    col.all_calendars_mut().insert(CAL_ID.to_string(), cal);

    let mut settings = Settings::new(xdg.get_config_home().unwrap().join("settings.toml"));
    settings.collections_mut().insert(COL_ID.to_string(), col);
    settings.write_to_file().expect("write settings");

    // Keep the TempDirs alive until the state is created, then let them drop; the state only needs
    // the files during construction (locale is loaded once into memory by State::new).
    let state = eventix_state::State::new(xdg).expect("State::new");
    Arc::new(tokio::sync::Mutex::new(state))
}

/// Builds a minimal axum `Router` wiring only the add-item endpoints.
///
/// This is sufficient for create-event and create-todo tests. The pages router is mounted at
/// `/pages/items/add` and the API items router at `/api/items`.
pub fn make_router(state: EventixState) -> Router {
    Router::new()
        .nest("/pages/items", eventix::pages::items::router(state.clone()))
        .nest("/api/items", eventix::api::items::router(state))
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

/// Reads the single `.ics` file written to `cal_dir` and returns it as a `CalFile`.
///
/// Panics if there is not exactly one `.ics` file in the directory.
pub fn read_created_ics(cal_dir: &Path) -> CalFile {
    let entries: Vec<_> = std::fs::read_dir(cal_dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("ics") {
                Some(p)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        entries.len(),
        1,
        "expected exactly 1 .ics file, found {}: {:?}",
        entries.len(),
        entries
    );

    let tz = chrono_tz::UTC;
    CalFile::new_from_file(Arc::new(CAL_ID.to_string()), entries[0].clone(), &tz).unwrap()
}

/// Asserts that no `.ics` file was created in `cal_dir`.
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

/// Asserts that the HTML response body contains a success info banner and no error banner.
pub fn assert_success(body: &str) {
    assert!(
        body.contains("ev_msg_info") || body.contains("info.event_added"),
        "expected success info banner in response, got:\n{body}"
    );
    assert!(
        !body.contains("ev_msg_error"),
        "expected no error banner in response, got:\n{body}"
    );
}

/// Asserts that the HTML response body contains an error banner and no success info banner.
pub fn assert_error(body: &str) {
    assert!(
        body.contains("ev_msg_error"),
        "expected error banner in response, got:\n{body}"
    );
}

/// Returns the first component from `cal_file`.
///
/// Panics if the file contains no components.
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
