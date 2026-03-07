use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

/// Path to the `tests/data/` directory that ships with the crate source.
pub fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

/// Copy one fixture file into `dest_dir` and return its full path.
pub fn copy_fixture(name: &str, dest_dir: &TempDir) -> PathBuf {
    let src = data_dir().join(name);
    let dst = dest_dir.path().join(name);
    fs::copy(&src, &dst).unwrap_or_else(|e| panic!("copy {name}: {e}"));
    dst
}

pub fn make_id(s: &str) -> Arc<String> {
    Arc::new(s.to_string())
}
