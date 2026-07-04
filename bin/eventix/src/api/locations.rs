// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{
    Json, Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::api::JsonError;

#[derive(Debug, Deserialize)]
pub struct Request {
    term: String,
}

#[derive(Debug, Serialize)]
struct Response(Vec<String>);

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/locations", get(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, JsonError> {
    let state = state.lock().await;
    let mut locations = state
        .store()
        .locations()
        .into_iter()
        .filter(|l| l.contains(&req.term))
        .collect::<Vec<_>>();
    locations.sort();
    Ok(Json(Response(locations)))
}
