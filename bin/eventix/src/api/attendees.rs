// Copyright (C) 2025 Nils Asmussen
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
        .route("/attendees", get(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, JsonError> {
    let state = state.lock().await;
    let mut contacts = state
        .store()
        .contacts()
        .iter()
        .filter(|(address, name)| name.contains(&req.term) || address.contains(&req.term))
        .map(|(address, name)| {
            if address == name {
                address.clone()
            } else {
                let address = match address.strip_prefix("mailto:") {
                    Some(addr) => addr,
                    None => address,
                };
                format!("{name} <{address}>")
            }
        })
        .collect::<Vec<_>>();
    contacts.sort();
    Ok(Json(Response(contacts)))
}
