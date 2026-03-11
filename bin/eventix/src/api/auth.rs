// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::{Router, routing::get};
use eventix_locale::Locale;
use eventix_state::EventixState;
use formatx::formatx;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::JsonError;
use crate::html::filters;

pub fn router(state: EventixState) -> Router {
    Router::new().route("/auth", get(handler)).with_state(state)
}

#[derive(Debug, Deserialize)]
struct Request {
    calendar: String,
    url: String,
}

#[derive(Debug, Serialize)]
struct Response {
    html: String,
}

#[derive(Template)]
#[template(path = "ajax/auth.htm")]
struct AuthTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
    error: String,
    url: String,
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, JsonError> {
    let locale = state.lock().await.locale();

    let error = formatx!(locale.translate("error.reauth_required"), &req.calendar).unwrap();
    let html = AuthTemplate {
        locale,
        error,
        url: req.url,
    }
    .render()
    .context("auth template")?;

    Ok(Json(Response { html }))
}
