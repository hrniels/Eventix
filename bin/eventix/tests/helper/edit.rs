// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::sync::Arc;

use eventix_ical::col::CalFile;

use crate::helper::CAL_ID;

/// Reads the `.ics` file in `cal_dir` whose stem matches `uid` exactly and returns it as a
/// `CalFile`.
///
/// Unlike `read_created_ics`, this works correctly when multiple `.ics` files exist in the
/// directory (e.g. after a Following-mode series split).
///
/// Panics if no matching file is found.
pub fn read_ics_by_uid(cal_dir: &Path, uid: &str) -> CalFile {
    let entries: Vec<_> = std::fs::read_dir(cal_dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            let p = e.path();
            let matches = p.extension().and_then(|s| s.to_str()) == Some("ics")
                && (p.file_stem().and_then(|s| s.to_str()) == Some(uid));
            if matches { Some(p) } else { None }
        })
        .collect();

    assert_eq!(
        entries.len(),
        1,
        "expected exactly 1 .ics file for uid '{uid}', found {}: {:?}",
        entries.len(),
        entries
    );

    let tz = chrono_tz::UTC;
    CalFile::new_from_file(Arc::new(CAL_ID.to_string()), entries[0].clone(), &tz).unwrap()
}

/// Asserts that the HTML response body indicates a successful edit (the edit form is re-rendered
/// without an error banner).
pub fn assert_success(body: &str) {
    assert!(
        body.contains("id=\"edit-form\""),
        "expected edit form in response, got:\n{body}"
    );
    assert!(
        !body.contains("ev_msg_error"),
        "expected no error banner in response, got:\n{body}"
    );
}

/// Returns the mtime of `path` in nanoseconds since the Unix epoch.
pub fn mtime_nanos(path: &Path) -> u128 {
    std::fs::metadata(path)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
