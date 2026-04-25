// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Context;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_state::EventixState;
use serde::Deserialize;

use crate::api::{JsonError, run_post};

#[derive(Debug, Deserialize)]
pub struct Params {
    col_id: String,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/delete", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Params>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    run_post(state, move |state| {
        Box::pin(run_delete_collection(state, req))
    })
    .await
}

async fn run_delete_collection(
    state: &mut eventix_state::State,
    req: Params,
) -> anyhow::Result<Json<()>> {
    eventix_state::State::delete_collection(state, &req.col_id)
        .await
        .context(format!("Unable to delete collection {}", req.col_id))?;

    if let Err(e) = state.settings().write_to_file() {
        tracing::warn!("Unable to save settings: {}", e);
    }

    eventix_state::State::refresh_store(state).await?;

    Ok(Json(()))
}
