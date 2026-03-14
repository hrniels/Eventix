// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use eventix_ical::objects::{CalDate, CalPartStat, EventLike};
use eventix_locale::{DateFlags, Locale, TimeFlags};
use eventix_state::EventixState;
use std::sync::Arc;

use super::Request;
use crate::pages::{Page, collections::Form, error::HTMLError, events::Events, tasks::Tasks};
use crate::{comps::syncer::SyncerTemplate, html::filters};

/// Full-page shell template. The form content is loaded separately via AJAX.
#[derive(Template)]
#[template(path = "pages/collections/add.htm")]
struct CollectionAddShellTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    /// The initial serialized query string to seed the first AJAX content request.
    request_query: String,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

/// Fragment-only template for the add-collection form, rendered by the AJAX content endpoint.
#[derive(Template)]
#[template(path = "pages/collections/add_content.htm")]
struct CollectionAddContentTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    prev: Option<&'a String>,
    name: String,
    syncer: SyncerTemplate<'a>,
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let page = super::new_page(&state).await;

    let st = state.lock().await;
    let locale = st.locale();
    let events = Events::new(&st, &locale);
    let tasks = Tasks::new(&st, &locale);

    let request_query = serde_qs::to_string(&req).unwrap_or_default();

    let html = CollectionAddShellTemplate {
        page,
        locale,
        request_query,
        events,
        tasks,
    }
    .render()
    .context("collections add shell template")?;

    Ok(Html(html))
}

/// Renders only the add-collection form fragment for the given request. Used by both the
/// AJAX content endpoint (GET) and the save handler (POST) to re-render the form.
pub async fn content_fragment(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = state.lock().await.locale();
    content(Page::default(), locale, State(state), Form::new(), req).await
}

/// Renders the add-collection form fragment with the given page state and form data.
/// Called by `content_fragment` for the initial GET and by `save::handler` after a POST.
pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(_state): State<EventixState>,
    form: Form,
    req: Request,
) -> Result<impl IntoResponse, HTMLError> {
    let html = CollectionAddContentTemplate {
        page,
        name: form.name.unwrap_or_default(),
        syncer: SyncerTemplate::new(locale.clone(), "syncer", form.syncer, None),
        prev: req.prev.as_ref(),
        locale,
    }
    .render()
    .context("collections add content template")?;

    Ok(Html(html))
}
