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
use eventix_ical::objects::{CalPartStat, EventLike};
use eventix_locale::{DateFlags, Locale, TimeFlags};
use eventix_state::EventixState;
use std::sync::Arc;

use crate::{
    html::filters,
    pages::{Page, error::HTMLError, events::Events, tasks::Tasks},
};

/// Fragment template for the sidebar containing the next-events and next-tasks boxes.
///
/// Rendered by the AJAX sidebar endpoint and injected into `#sidebar-content` on the client.
#[derive(Template)]
#[template(path = "sidebar.htm")]
struct SidebarTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

/// Renders the sidebar fragment (next events and next tasks boxes).
///
/// Used by the AJAX endpoint at `/pages/sidebar/content` to lazy-load the right-hand
/// sidebar without blocking the initial page render.
async fn content(State(state): State<EventixState>) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(&state).await;

    let st = state.lock().await;

    let locale = st.locale();
    let events = Events::new(&st, &locale);
    let tasks = Tasks::new(&st, &locale);

    let html = SidebarTemplate {
        page,
        locale,
        events,
        tasks,
    }
    .render()
    .context("sidebar template")?;

    Ok(Html(html))
}

/// Returns the router for the sidebar, exposing only the AJAX content endpoint.
pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/content", get(content))
        .with_state(state)
}
