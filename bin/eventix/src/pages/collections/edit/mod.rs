// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod index;
mod save;

use axum::{
    Router,
    routing::{get, post},
};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use super::Page;

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    col_id: String,
    prev: Option<String>,
}

pub async fn new_page(state: &EventixState) -> Page {
    Page::new(state).await
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/", get(self::index::handler))
        .route("/", post(self::save::handler))
        .with_state(state)
}
