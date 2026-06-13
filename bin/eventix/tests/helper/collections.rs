// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix::api::HTMLResponse;

/// Asserts that the HTML response body contains a success info banner and no error banner.
pub fn assert_success(resp: &HTMLResponse) {
    assert!(
        resp.html.contains("ev_msg_info"),
        "expected success info banner in response, got:\n{}",
        resp.html
    );
    assert!(
        !resp.html.contains("ev_msg_error"),
        "expected no error banner in response, got:\n{}",
        resp.html
    );
}
