// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod index;
mod save;

use axum::{
    Router,
    extract::{RawQuery, State},
    routing::{get, post},
};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::pages::{Page, shell};

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    prev: Option<String>,
}

pub async fn new_page(state: &EventixState) -> Page {
    Page::new(state).await
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route(
            "/",
            get(
                |State(state): State<EventixState>, RawQuery(raw): RawQuery| async move {
                    shell::handler(state, raw, "collections/add").await
                },
            ),
        )
        .route("/", post(self::save::handler))
        .route("/content", get(self::index::content))
        .with_state(state)
}
