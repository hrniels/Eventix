use anyhow::{Context, Result};
use askama::Template;
use axum::response::{IntoResponse, Json};
use axum::{Router, routing::get};
use serde::Serialize;
use std::sync::Arc;

use crate::html::filters;
use crate::locale::{self, Locale};
use crate::pages::error::HTMLError;
use crate::state::EventixState;

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

async fn handler() -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();

    let html = HelpTemplate { locale }.render().context("help template")?;

    Ok(Json(Response { html }))
}
