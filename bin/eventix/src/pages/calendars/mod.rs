// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod index;

use axum::{Router, routing::get};
use eventix_state::EventixState;

use super::Page;

pub async fn new_page(state: &EventixState) -> Page {
    Page::new(state).await
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/", get(self::index::handler))
        .with_state(state)
}
