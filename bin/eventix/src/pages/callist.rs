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
use eventix_state::EventixState;

use crate::pages::{Page, error::HTMLError};

/// Fragment template for the calendar toggle button list in the topbar.
///
/// Rendered by the AJAX callist endpoint and injected into `#callist-content` on the client.
#[derive(Template)]
#[template(path = "comps/callist.htm")]
struct CalListTemplate {
    page: Page,
}

/// Renders the calendar list fragment (per-calendar toggle buttons and hover scripts).
///
/// Used by the AJAX endpoint at `/pages/callist/content` to lazy-load the calendar toggle buttons
/// in the topbar without blocking the initial page render.
async fn content(State(state): State<EventixState>) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(&state).await;

    let html = CalListTemplate { page }
        .render()
        .context("callist template")?;

    Ok(Html(html))
}

/// Returns the router for the calendar list, exposing only the AJAX content endpoint.
pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/content", get(content))
        .with_state(state)
}
