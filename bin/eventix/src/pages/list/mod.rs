// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod index;

use axum::{
    Router,
    extract::{RawQuery, State},
    routing::get,
};
use eventix_state::EventixState;

use crate::pages::shell;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route(
            "/",
            get(
                |State(state): State<EventixState>, RawQuery(raw): RawQuery| async move {
                    shell::handler(state, raw, "list/shell").await
                },
            ),
        )
        .route("/shell/content", get(self::index::shell_fragment))
        .route("/content", get(self::index::content))
        .with_state(state)
}
