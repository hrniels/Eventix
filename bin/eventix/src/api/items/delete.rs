// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Context;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::api::{JsonError, run_post};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/delete", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(form): Query<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    run_post(state, move |state| Box::pin(run_delete(state, form))).await
}

async fn run_delete(
    state: &mut eventix_state::State,
    form: Request,
) -> anyhow::Result<Json<Response>> {
    let file = state
        .store_mut()
        .try_file_by_id_mut(&form.uid)
        .context(format!("Unable to find component with uid '{}'", form.uid))?;

    let src = file.directory().clone();
    state
        .store_mut()
        .try_directory_mut(&src)
        .map_err(anyhow::Error::from)?
        .delete_by_uid(&form.uid)
        .with_context(|| format!("Unable to delete item with uid {}", form.uid))?;

    Ok(Json(Response {}))
}
