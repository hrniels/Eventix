use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;
use xdg::BaseDirectories;

/// Returns the path to the project-level `data/` directory (two levels above `libs/state`).
///
/// Integration tests that need locale files (e.g. `English.toml`) should point `XDG_DATA_HOME`
/// at this directory so that `eventix_locale::new` can find them.
#[allow(unused)]
pub fn project_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..") // libs/
        .join("..") // project root
        .join("data")
}

// Integration tests should reuse the crate-level helper so locking is consistent with unit
// tests. `eventix_state::with_test_xdg` acquires a global lock while setting env vars and
// constructs a `BaseDirectories` snapshot.
fn locked_xdg(data: &Path, config: &Path) -> Arc<BaseDirectories> {
    Arc::new(eventix_state::with_test_xdg(data, config))
}

/// Creates an `Arc<BaseDirectories>` rooted inside `root`.
///
/// Sets `XDG_CONFIG_HOME` and `XDG_DATA_HOME` to subdirectories of `root` and also creates them
/// so that callers can immediately place files there. Returns the `Arc<BaseDirectories>`.
#[allow(unused)]
pub fn make_xdg(root: &Path) -> Arc<BaseDirectories> {
    let config = root.join("config");
    let data = root.join("data");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    locked_xdg(&data, &config)
}

/// Creates an `Arc<BaseDirectories>` whose `XDG_DATA_HOME` points to the project `data/`
/// directory so that locale files can be found.
///
/// `XDG_CONFIG_HOME` is set to a fresh temporary subdirectory inside `tmp`.
#[allow(unused)]
pub fn make_xdg_with_real_data(tmp: &TempDir) -> Arc<BaseDirectories> {
    let config = tmp.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    locked_xdg(&project_data_dir(), &config)
}

/// Creates an `Arc<BaseDirectories>` with both `XDG_CONFIG_HOME` and `XDG_DATA_HOME` pointing
/// inside `tmp`, with the project locale files symlinked in so that `eventix_locale::new` works.
///
/// This is the preferred helper for tests that need both writable data directories (e.g. to
/// create vdirsyncer log files) and locale support.
#[allow(unused)]
pub fn make_xdg_with_locale(tmp: &TempDir) -> Arc<BaseDirectories> {
    let config = tmp.path().join("config");
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    // Symlink the project locale directory into the temp data dir so locale loading succeeds.
    let src_locale = project_data_dir().join("locale");
    let dst_locale = data.join("locale");
    if !dst_locale.exists() {
        std::os::unix::fs::symlink(&src_locale, &dst_locale).unwrap();
    }

    locked_xdg(&data, &config)
}

/// Builds a minimal enabled `CalendarSettings` with the given folder and display name.
#[allow(unused)]
pub fn make_cal_settings(
    enabled: bool,
    folder: &str,
    name: &str,
) -> eventix_state::CalendarSettings {
    let mut cal = eventix_state::CalendarSettings::default();
    cal.set_enabled(enabled);
    cal.set_folder(folder.to_string());
    cal.set_name(name.to_string());
    cal
}

/// Builds a `CollectionSettings` backed by a `FileSystem` syncer rooted at `path`.
#[allow(unused)]
pub fn make_filesystem_col(path: &Path) -> eventix_state::CollectionSettings {
    eventix_state::CollectionSettings::new(eventix_state::SyncerType::FileSystem {
        path: path.to_string_lossy().into_owned(),
    })
}

/// Wraps `s` in an `Arc<String>`, for use in tests that require an `Arc<String>` ID.
#[allow(unused)]
pub fn make_id(s: &str) -> Arc<String> {
    Arc::new(s.to_string())
}
