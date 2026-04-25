// Copyright (C) 2025 Nils Asmussen
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
use tracing::warn;

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
    {
        let misc = state.misc_mut();
        misc.set_locale_type(req.lang);
        if let Err(e) = misc.write_to_file() {
            warn!("Unable to save misc state: {}", e);
        }
    }

    state.reload_locale().context("Loading locale")?;
    generated::invalidate().await;

    Ok(Json(()))
}
