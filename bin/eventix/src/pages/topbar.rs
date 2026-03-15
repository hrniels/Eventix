// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::{
    Router,
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
};
use eventix_locale::{Locale, TimeFlags};
use eventix_state::EventixState;
use std::sync::Arc;

use crate::{
    html::filters,
    pages::{Page, error::HTMLError},
};

/// Fragment template for the topbar containing the search form, calendar toggles, and nav links.
///
/// Rendered by the AJAX topbar endpoint and injected into `#topbar-content` on the client.
#[derive(Template)]
#[template(path = "topbar.htm")]
struct TopbarTemplate {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
}

/// Renders the topbar fragment (search form, calendar toggles, and navigation links).
///
/// Used by the AJAX endpoint at `/pages/topbar/content` to lazy-load the topbar without blocking
/// the initial page render.
async fn content(State(state): State<EventixState>) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(&state).await;
    let locale = state.lock().await.locale();

    let html = TopbarTemplate { page, locale }
        .render()
        .context("topbar template")?;

    Ok(Html(html))
}

/// Returns the router for the topbar, exposing only the AJAX content endpoint.
pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/content", get(content))
        .with_state(state)
}
