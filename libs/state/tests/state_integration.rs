//! Integration tests for [`eventix_state::State`] that require filesystem access.
//!
//! Each test gets its own temporary directory and sets `XDG_CONFIG_HOME` / `XDG_DATA_HOME`
//! accordingly. Because these variables are process-wide, tests in this file share a single
//! test binary and run sequentially (the default for integration test binaries).

use std::sync::Arc;

use tempfile::TempDir;

use eventix_state::{
    CollectionSettings, Settings, State, SyncerType, load_from_file, write_to_file,
};

mod common;
use common::{make_cal_settings, make_filesystem_col, make_id, make_xdg_with_real_data};

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
