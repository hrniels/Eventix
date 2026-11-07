// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::routing::post;
use eventix_locale::LocaleType;
use eventix_state::EventixState;
use serde::Deserialize;

use crate::api::{JsonError, run_post};
use crate::generated;

#[derive(Debug, Deserialize)]
pub struct Request {
    lang: LocaleType,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/setlang", post(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, JsonError> {
    run_post(state, move |state| Box::pin(run_setlang(state, req))).await
}

async fn run_setlang(state: &mut eventix_state::State, req: Request) -> anyhow::Result<Json<()>> {
    let mut preferences = state.settings().preferences();
    preferences.locale_type = req.lang;
    state
        .save_preferences(preferences)
        .context("Saving preferences")?;
    generated::invalidate().await;

    Ok(Json(()))
}
