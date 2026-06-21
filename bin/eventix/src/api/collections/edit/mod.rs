// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod index;

use axum::{
    Router,
    routing::{get, post},
};
use eventix_state::EventixState;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/", get(index::handler))
        .route("/", post(index::save_handler))
        .with_state(state)
}
