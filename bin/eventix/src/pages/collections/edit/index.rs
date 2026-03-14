// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result, anyhow};
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

/// Fragment-only template for the edit-collection form, rendered by the AJAX content endpoint.
#[derive(Template)]
#[template(path = "pages/collections/edit_content.htm")]
struct CollectionEditContentTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    col_id: &'a String,
    prev: Option<&'a String>,
    syncer: SyncerTemplate<'a>,
}

/// Renders only the edit-collection form fragment for the given request. Used by both the
/// AJAX content endpoint (GET) and the save handler (POST) to re-render the form.
pub async fn content_fragment(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = state.lock().await.locale();
    content(Page::default(), locale, State(state), None, req).await
}

/// Renders the edit-collection form fragment with the given page state and form data.
/// Called by `content_fragment` for the initial GET and by `save::handler` after a POST.
pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<EventixState>,
    form: Option<Form>,
    req: Request,
) -> Result<impl IntoResponse, HTMLError> {
    let state = state.lock().await;

    let form = if let Some(form) = form {
        form
    } else {
        let col = state
            .settings()
            .collections()
            .get(&req.col_id)
            .ok_or_else(|| anyhow!("No collection with id {}", req.col_id))?;
        Form::new_from(col)
    };

    let syncer = form.syncer_type();

    let html = CollectionEditContentTemplate {
        page,
        syncer: SyncerTemplate::new(locale.clone(), "syncer", form.syncer, syncer),
        col_id: &req.col_id,
        prev: req.prev.as_ref(),
        locale,
    }
    .render()
    .context("collections edit content template")?;

    Ok(Html(html))
}
