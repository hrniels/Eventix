// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

/// Asserts that the HTML response body contains a success info banner and no error banner.
pub fn assert_success(body: &str) {
    assert!(
        body.contains("ev_msg_info"),
        "expected success info banner in response, got:\n{body}"
    );
    assert!(
        !body.contains("ev_msg_error"),
        "expected no error banner in response, got:\n{body}"
    );
}
