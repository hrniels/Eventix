// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix::api::HTMLResponse;

/// Asserts that the response indicates success: no errors and empty html (since the API
/// returns an empty body on success rather than rendering a banner).
pub fn assert_success(resp: &HTMLResponse) {
    assert!(
        resp.errors.is_empty(),
        "expected no errors in response, got: {:?}",
        resp.errors
    );
    assert!(
        resp.html.is_empty(),
        "expected empty html on success, got: {}",
        resp.html
    );
}
