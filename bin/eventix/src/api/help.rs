use anyhow::{Context, Result};
use askama::Template;
use axum::extract::State;
use axum::response::{IntoResponse, Json};
use axum::{Router, routing::get};
use eventix_locale::Locale;
use eventix_state::EventixState;
use serde::Serialize;
use std::sync::Arc;

use crate::api::JsonError;
use crate::html::filters;

pub fn router(state: EventixState) -> Router {
    Router::new().route("/help", get(handler)).with_state(state)
}

#[derive(Debug, Serialize)]
struct Response {
    html: String,
}

#[derive(Template)]
#[template(path = "ajax/help.htm")]
struct HelpTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
}

async fn handler(State(state): State<EventixState>) -> Result<impl IntoResponse, JsonError> {
    let locale = state.lock().await.settings().locale();

    let html = HelpTemplate { locale }.render().context("help template")?;

    Ok(Json(Response { html }))
}
