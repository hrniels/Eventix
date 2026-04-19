// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono_tz::Tz;
use eventix_state::{CalendarSettings, CollectionSettings, EventixState, Settings, SyncerType};
use tempfile::TempDir;
use tokio::runtime::Runtime;
use tower::ServiceExt;

#[allow(dead_code)]
pub const COL_ID: &str = "bench-col";
pub const CAL_ID: &str = "bench-cal";
pub const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/data");
const SEED_FIXTURE_COUNT: u64 = 10;
pub const FIXTURE_COUNT: u64 = 1_000;

#[allow(dead_code)]
pub fn build_runtime() -> Runtime {
    Runtime::new().expect("create tokio runtime")
}

#[allow(dead_code)]
pub fn build_calendar_dir() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().expect("create tempdir");
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).expect("create calendar dir");
    populate_fixtures(Path::new(FIXTURE_DIR), &cal_dir);
    (tmp, cal_dir)
}

#[allow(dead_code)]
pub fn benchmark_local_tz() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse::<Tz>().ok())
        .unwrap_or(Tz::UTC)
}

#[allow(dead_code)]
pub fn build_state() -> (EventixState, TempDir) {
    let tmp = TempDir::new().expect("create tempdir");
    let data_home = tmp.path().join("data");
    let config_home = tmp.path().join("config");
    let locale_dir = data_home.join("locale");
    let cal_root = data_home.join("collections");
    let cal_dir = cal_root.join(CAL_ID);

    std::fs::create_dir_all(&locale_dir).expect("create locale dir");
    std::fs::create_dir_all(&cal_dir).expect("create calendar dir");
    std::fs::create_dir_all(&config_home).expect("create config dir");

    std::fs::write(
        locale_dir.join("English.toml"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/locale/English.toml"
        )),
    )
    .expect("write English locale");

    populate_fixtures(Path::new(FIXTURE_DIR), &cal_dir);

    let xdg = Arc::new(eventix_state::with_test_xdg(&data_home, &config_home));
    let mut settings = Settings::new(xdg.get_config_home().unwrap().join("settings.toml"));
    let mut col = CollectionSettings::new(SyncerType::FileSystem {
        path: cal_root.to_string_lossy().into_owned(),
    });
    let mut cal = CalendarSettings::default();
    cal.set_enabled(true);
    cal.set_folder(CAL_ID.to_string());
    cal.set_name("Benchmark Calendar".to_string());
    col.all_calendars_mut().insert(CAL_ID.to_string(), cal);
    settings.collections_mut().insert(COL_ID.to_string(), col);
    settings.write_to_file().expect("write settings");

    let state = eventix_state::State::new(xdg).expect("create state");
    (Arc::new(tokio::sync::Mutex::new(state)), tmp)
}

pub fn populate_fixtures(src: &Path, dst: &Path) {
    let mut seeds = src
        .read_dir()
        .expect("read fixture dir")
        .map(|entry| entry.expect("read fixture entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("ics"))
        .collect::<Vec<_>>();
    seeds.sort();

    assert_eq!(
        seeds.len() as u64,
        SEED_FIXTURE_COUNT,
        "expected {SEED_FIXTURE_COUNT} seed ICS fixtures"
    );

    for index in 0..FIXTURE_COUNT as usize {
        let seed_path = &seeds[index % seeds.len()];
        let content = std::fs::read_to_string(seed_path).expect("read seed fixture");
        let stem = seed_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("fixture stem");
        let unique = format!("{stem}-{index:04}");
        let content = content.replace("UID:", &format!("UID:{unique}-"));

        std::fs::write(dst.join(format!("{unique}.ics")), content).expect("write bench fixture");
    }
}

#[allow(dead_code)]
pub async fn run_request(router: Router, uri: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("build request");

    let resp = router.oneshot(req).await.expect("run request");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let body = String::from_utf8(bytes.to_vec()).expect("utf8 body");
    (status, body)
}
