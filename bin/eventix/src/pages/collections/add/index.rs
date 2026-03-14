// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use eventix_locale::Locale;
use eventix_state::EventixState;
use std::sync::Arc;

use super::Request;
use crate::pages::{Page, collections::Form, error::HTMLError};
use crate::{comps::syncer::SyncerTemplate, html::filters};

/// Fragment-only template for the add-collection form, rendered by the AJAX content endpoint.
#[derive(Template)]
#[template(path = "pages/collections/add.htm")]
struct CollectionAddTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    prev: Option<&'a String>,
    name: String,
    syncer: SyncerTemplate<'a>,
}

/// Renders only the add-collection form fragment for the given request. Used by the AJAX content
/// endpoint (GET).
pub async fn content(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = state.lock().await.locale();
    content_with(Page::default(), locale, State(state), Form::new(), req).await
}

/// Renders the add-collection form fragment with the given page state and form data.
/// Called by `content` for the initial GET and by `save::handler` after a POST.
pub async fn content_with(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(_state): State<EventixState>,
    form: Form,
    req: Request,
) -> Result<impl IntoResponse, HTMLError> {
    let html = CollectionAddTemplate {
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
