//! Integration tests for [`eventix_state`] settings types that require filesystem access.

use tempfile::TempDir;

use eventix_state::{CollectionSettings, Settings, SyncerType};

mod common;
use common::{make_cal_settings, make_xdg};

// --- helpers ---

fn make_filesystem_syncer() -> SyncerType {
    SyncerType::FileSystem {
        path: "/data/calendars".to_string(),
    }
}

// --- CollectionSettings ---

#[test]
fn collection_settings_path_and_log_file() {
    let tmpdir = TempDir::new().unwrap();
    let xdg = make_xdg(tmpdir.path());

    let col = CollectionSettings::new(make_filesystem_syncer());
    assert_eq!(
        col.path(&xdg, "mycol"),
        std::path::PathBuf::from("/data/calendars")
    );

    let log = col.log_file(&xdg, "mycol");
    assert!(log.ends_with("vdirsyncer/mycol.log"));
}

// --- Settings ---

#[test]
fn settings_load_from_file_missing() {
    // When no settings.toml exists, load_from_file returns an empty Settings.
    let tmpdir = TempDir::new().unwrap();
    let xdg = make_xdg(tmpdir.path());

    let settings = Settings::load_from_file(&xdg).expect("load must succeed even without file");
    assert!(settings.collections().is_empty());
}

#[test]
fn settings_write_and_load_round_trip() {
    let tmpdir = TempDir::new().unwrap();
    let xdg = make_xdg(tmpdir.path());

    // Build settings with one collection and one calendar.
    let config_home = tmpdir.path().join("config");
    let path = config_home.join("settings.toml");

    let mut original = Settings::new(path);
    let mut col = CollectionSettings::new(make_filesystem_syncer());
    col.all_calendars_mut().insert(
        "cal-rt".to_string(),
        make_cal_settings(true, "rt-folder", "RT Cal"),
    );
    original.collections_mut().insert("col-rt".to_string(), col);
    original
        .write_to_file()
        .expect("write_to_file must succeed");

    // Load them back via the XDG path.
    let loaded = Settings::load_from_file(&xdg).expect("load_from_file must succeed");
    assert!(loaded.collections().contains_key("col-rt"));
    let loaded_col = loaded.collections().get("col-rt").unwrap();
    let loaded_cal = loaded_col.all_calendars().get("cal-rt").unwrap();
    assert_eq!(loaded_cal.name(), "RT Cal");
    assert_eq!(loaded_cal.folder(), "rt-folder");
    assert!(loaded_cal.enabled());
}
