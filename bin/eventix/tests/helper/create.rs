// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::Path, sync::Arc};

use eventix_ical::col::CalFile;

use crate::helper::CAL_ID;

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
