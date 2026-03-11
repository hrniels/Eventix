// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for [`eventix_state::State`] that require filesystem access.
//!
//! Each test gets its own temporary directory and sets `XDG_CONFIG_HOME` / `XDG_DATA_HOME`
//! accordingly. Because these variables are process-wide, tests in this file share a single
//! test binary and run sequentially (the default for integration test binaries).

use std::sync::Arc;

use tempfile::TempDir;

use eventix_state::{
    CollectionSettings, Settings, State, SyncColResult, SyncerType, load_from_file, write_to_file,
};

mod common;
use common::{
    make_cal_settings, make_filesystem_col, make_id, make_xdg_with_locale, make_xdg_with_real_data,
};

// --- State::new ---

#[test]
fn state_new_empty_xdg_dir() {
    // When no settings.toml, no misc.toml, and no personal-alarms directory exist,
    // State::new must succeed and return an empty state.
    let tmp = TempDir::new().unwrap();
    // XDG_DATA_HOME must point at the project data/ dir so the locale file can be found.
    let xdg = make_xdg_with_real_data(&tmp);

    let state = State::new(xdg).expect("State::new must succeed with an empty XDG dir");

    assert!(state.store().directories().is_empty());
    assert!(state.settings().collections().is_empty());
}

// --- reload_locale ---

#[test]
fn reload_locale_succeeds_with_real_locale_files() {
    // reload_locale must succeed when XDG_DATA_HOME points at the project data/ directory.
    let tmp = TempDir::new().unwrap();
    let xdg = make_xdg_with_real_data(&tmp);

    let mut state = State::new(xdg).expect("State::new must succeed");
    state
        .reload_locale()
        .expect("reload_locale must succeed when locale files are present");
}

// --- refresh_store: add and rename ---

#[test]
fn refresh_store_adds_new_calendar_from_settings() {
    // Start with an empty store. Add a collection with one calendar to settings pointing at a
    // real (empty) directory on disk. After refresh_store the store must contain the calendar.
    let tmp = TempDir::new().unwrap();
    let xdg = make_xdg_with_real_data(&tmp);

    // Create the calendar directory on disk.
    let cal_dir = tmp.path().join("col").join("mycal");
    std::fs::create_dir_all(&cal_dir).unwrap();

    let mut state = State::new(Arc::clone(&xdg)).expect("State::new must succeed");

    let mut col = make_filesystem_col(&tmp.path().join("col"));
    col.all_calendars_mut().insert(
        "cal-a".to_string(),
        make_cal_settings(true, "mycal", "My Cal"),
    );
    state
        .settings_mut()
        .collections_mut()
        .insert("col1".to_string(), col);

    // Run refresh_store; because it is async we use a tokio runtime.
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::refresh_store(&mut state))
        .expect("refresh_store must succeed");

    assert_eq!(state.store().directories().len(), 1);
    let dir = state
        .store()
        .directory(&make_id("cal-a"))
        .expect("cal-a must be in store");
    assert_eq!(dir.name(), "My Cal");
}

#[test]
fn refresh_store_renames_existing_calendar() {
    // Pre-populate the store with a calendar, then change its name in settings. After
    // refresh_store the directory must carry the new name.
    let tmp = TempDir::new().unwrap();
    let xdg = make_xdg_with_real_data(&tmp);

    let cal_dir = tmp.path().join("col").join("folder");
    std::fs::create_dir_all(&cal_dir).unwrap();

    let mut state = State::new(Arc::clone(&xdg)).expect("State::new must succeed");

    // Add the calendar via refresh_store first (simulates pre-existing state).
    let mut col = make_filesystem_col(&tmp.path().join("col"));
    col.all_calendars_mut().insert(
        "cal-r".to_string(),
        make_cal_settings(true, "folder", "Old Name"),
    );
    state
        .settings_mut()
        .collections_mut()
        .insert("col1".to_string(), col);

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::refresh_store(&mut state))
        .expect("first refresh_store must succeed");

    // Now rename the calendar in settings and re-run.
    state
        .settings_mut()
        .collections_mut()
        .get_mut("col1")
        .unwrap()
        .all_calendars_mut()
        .get_mut("cal-r")
        .unwrap()
        .set_name("New Name".to_string());

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::refresh_store(&mut state))
        .expect("second refresh_store must succeed");

    let dir = state
        .store()
        .directory(&make_id("cal-r"))
        .expect("cal-r must still be in store");
    assert_eq!(dir.name(), "New Name");
}

#[test]
fn refresh_store_removes_calendar_absent_from_settings() {
    // Pre-populate the store with a calendar that is *not* present in settings.
    // After refresh_store the store must be empty.
    let tmp = TempDir::new().unwrap();
    let xdg = make_xdg_with_real_data(&tmp);

    // Create a real dir so the initial load succeeds.
    let cal_dir = tmp.path().join("col").join("stale");
    std::fs::create_dir_all(&cal_dir).unwrap();

    let mut state = State::new(Arc::clone(&xdg)).expect("State::new must succeed");

    // Add a calendar to settings and do a first refresh so the store is populated.
    let mut col = make_filesystem_col(&tmp.path().join("col"));
    col.all_calendars_mut().insert(
        "stale-cal".to_string(),
        make_cal_settings(true, "stale", "Stale"),
    );
    state
        .settings_mut()
        .collections_mut()
        .insert("col1".to_string(), col);

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::refresh_store(&mut state))
        .expect("first refresh_store must succeed");
    assert_eq!(state.store().directories().len(), 1);

    // Remove the calendar from settings, then refresh again.
    state
        .settings_mut()
        .collections_mut()
        .get_mut("col1")
        .unwrap()
        .all_calendars_mut()
        .remove("stale-cal");

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::refresh_store(&mut state))
        .expect("second refresh_store must succeed");

    assert!(
        state.store().directories().is_empty(),
        "stale calendar must have been removed from the store"
    );
}

// --- State sync wrappers ---

/// Builds a minimal `State` that has one FS-backed collection with one calendar.
///
/// The XDG data directory is pointed at `tmp` so that the vdirsyncer log subdirectory can be
/// created by the syncer infrastructure. The calendar directory itself is also created so that
/// `load_calendar` succeeds when `reload_from_file` is called. A symlink to the project locale
/// directory is created inside the temp data dir so that locale loading succeeds.
fn make_fs_state_with_xdg(tmp: &TempDir, col_id: &str, cal_id: &str, folder: &str) -> State {
    let xdg = make_xdg_with_locale(tmp);
    // Create the vdirsyncer log directory expected by the sync machinery.
    std::fs::create_dir_all(tmp.path().join("data/vdirsyncer")).unwrap();
    // Create the calendar folder so `load_calendar` can scan it.
    let cal_folder = tmp.path().join("data").join(col_id).join(folder);
    std::fs::create_dir_all(&cal_folder).unwrap();

    let mut state = State::new(Arc::clone(&xdg)).expect("State::new must succeed");

    let mut col = make_filesystem_col(&tmp.path().join("data").join(col_id));
    col.all_calendars_mut().insert(
        cal_id.to_string(),
        make_cal_settings(true, folder, "Test Cal"),
    );
    state
        .settings_mut()
        .collections_mut()
        .insert(col_id.to_string(), col);

    state
}

#[test]
fn state_discover_collection_with_fs_syncer() {
    let tmp = TempDir::new().unwrap();
    let mut state = make_fs_state_with_xdg(&tmp, "col1", "cal1", "mycal");

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::discover_collection(
            &mut state,
            &"col1".to_string(),
            None,
        ))
        .expect("discover_collection must succeed");

    assert_eq!(
        result.collections.get("col1"),
        Some(&SyncColResult::Success(false)),
        "FS discover must succeed with no changes"
    );
}

#[test]
fn state_sync_collection_with_fs_syncer() {
    let tmp = TempDir::new().unwrap();
    let mut state = make_fs_state_with_xdg(&tmp, "col1", "cal1", "mycal");

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::sync_collection(
            &mut state,
            &"col1".to_string(),
            None,
        ))
        .expect("sync_collection must succeed");

    assert_eq!(
        result.collections.get("col1"),
        Some(&SyncColResult::Success(false))
    );
    assert!(!result.changed);
}

#[test]
fn state_sync_all_updates_last_reload() {
    let tmp = TempDir::new().unwrap();
    let mut state = make_fs_state_with_xdg(&tmp, "col1", "cal1", "mycal");

    let before = state.last_reload();

    // Sleep briefly so the timestamp has a chance to advance.
    std::thread::sleep(std::time::Duration::from_millis(5));

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::sync_all(&mut state, None))
        .expect("sync_all must succeed");

    assert!(
        state.last_reload() >= before,
        "last_reload must be updated after sync_all"
    );
}

#[test]
fn state_reload_collection_reloads_store() {
    let tmp = TempDir::new().unwrap();
    let mut state = make_fs_state_with_xdg(&tmp, "col1", "cal1", "mycal");

    // Populate the store by running a first refresh.
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::refresh_store(&mut state))
        .expect("refresh_store must succeed");
    assert_eq!(
        state.store().directories().len(),
        1,
        "store must have one calendar"
    );

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::reload_collection(
            &mut state,
            &"col1".to_string(),
            None,
        ))
        .expect("reload_collection must succeed");

    assert_eq!(
        result.collections.get("col1"),
        Some(&SyncColResult::Success(false))
    );
    // The calendar must still be present in the store after the reload.
    assert!(
        state.store().directory(&make_id("cal1")).is_some(),
        "cal1 must be in the store after reload_collection"
    );
}

#[test]
fn state_delete_collection_removes_from_settings() {
    let tmp = TempDir::new().unwrap();
    let mut state = make_fs_state_with_xdg(&tmp, "col1", "cal1", "mycal");

    assert!(
        state.settings().collections().contains_key("col1"),
        "col1 must be in settings before delete"
    );

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::delete_collection(&mut state, &"col1".to_string()))
        .expect("delete_collection must succeed");

    assert!(
        !state.settings().collections().contains_key("col1"),
        "col1 must be removed from settings after delete_collection"
    );
}

#[test]
fn state_delete_calendar_removes_from_settings() {
    let tmp = TempDir::new().unwrap();
    let mut state = make_fs_state_with_xdg(&tmp, "col1", "cal1", "mycal");

    assert!(
        state
            .settings()
            .collections()
            .get("col1")
            .unwrap()
            .all_calendars()
            .contains_key("cal1"),
        "cal1 must be in settings before delete"
    );

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::delete_calendar(
            &mut state,
            &"col1".to_string(),
            &"cal1".to_string(),
        ))
        .expect("delete_calendar must succeed");

    assert!(
        !state
            .settings()
            .collections()
            .get("col1")
            .unwrap()
            .all_calendars()
            .contains_key("cal1"),
        "cal1 must be removed from settings after delete_calendar"
    );
}

#[test]
fn state_reload_calendar_reloads_store() {
    let tmp = TempDir::new().unwrap();
    let mut state = make_fs_state_with_xdg(&tmp, "col1", "cal1", "mycal");

    // Populate the store first.
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::refresh_store(&mut state))
        .expect("refresh_store must succeed");
    assert_eq!(state.store().directories().len(), 1);

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(State::reload_calendar(
            &mut state,
            &"col1".to_string(),
            &"cal1".to_string(),
            None,
        ))
        .expect("reload_calendar must succeed");

    assert_eq!(
        result.collections.get("col1"),
        Some(&SyncColResult::Success(false))
    );
    assert_eq!(result.calendars.get("cal1"), Some(&false));
    // The calendar must still be present in the store after the per-calendar reload.
    assert!(
        state.store().directory(&make_id("cal1")).is_some(),
        "cal1 must be in the store after reload_calendar"
    );
}

// --- load_from_file / write_to_file ---

#[test]
fn load_write_round_trip() {
    // A value serialised with write_to_file must be faithfully recovered by load_from_file.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("state.toml");

    let mut settings = Settings::new(path.clone());
    let mut col = CollectionSettings::new(SyncerType::FileSystem {
        path: "/tmp/cals".to_string(),
    });
    col.all_calendars_mut()
        .insert("cal-rt".to_string(), make_cal_settings(true, "rt", "RT"));
    settings.collections_mut().insert("col-rt".to_string(), col);

    write_to_file(&path, &settings).expect("write_to_file must succeed");

    let loaded: Settings = load_from_file(&path).expect("load_from_file must succeed");
    assert!(loaded.collections().contains_key("col-rt"));
    let loaded_cal = loaded
        .collections()
        .get("col-rt")
        .unwrap()
        .all_calendars()
        .get("cal-rt")
        .unwrap();
    assert_eq!(loaded_cal.name(), "RT");
    assert!(loaded_cal.enabled());
}

#[test]
fn load_from_file_missing_returns_error() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("does_not_exist.toml");

    let result: anyhow::Result<Settings> = load_from_file(&path);
    assert!(result.is_err(), "load_from_file on missing file must fail");
}

#[test]
fn write_to_file_missing_dir_returns_error() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("nonexistent_dir").join("file.toml");

    let settings = Settings::new(path.clone());
    let result = write_to_file(&path, &settings);
    assert!(result.is_err(), "write_to_file into missing dir must fail");
}

#[test]
fn write_to_file_overwrites_existing() {
    // Writing twice to the same file must truncate and overwrite the previous contents.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("overwrite.toml");

    let settings1 = Settings::new(path.clone());
    write_to_file(&path, &settings1).expect("first write must succeed");

    let mut settings2 = Settings::new(path.clone());
    settings2.collections_mut().insert(
        "new-col".to_string(),
        make_filesystem_col(std::path::Path::new("/tmp")),
    );
    write_to_file(&path, &settings2).expect("second write must succeed");

    let loaded: Settings = load_from_file(&path).expect("reload must succeed");
    assert!(
        loaded.collections().contains_key("new-col"),
        "second write must have overwritten the first"
    );
}
