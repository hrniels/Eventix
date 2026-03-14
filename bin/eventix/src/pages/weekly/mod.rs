// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod index;

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
                    shell::handler(state, raw, "weekly", "weekly-content").await
                },
            ),
        )
        .route("/content", get(self::index::content))
        .with_state(state)
}
