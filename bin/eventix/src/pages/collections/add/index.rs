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

#[derive(Template)]
#[template(path = "pages/collections/add.htm")]
struct CollectionAddTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    prev: Option<&'a String>,
    name: String,
    syncer: SyncerTemplate<'a>,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = state.lock().await.locale();

    content(
        super::new_page(&state).await,
        locale.clone(),
        State(state),
        Form::new(),
        req,
    )
    .await
}

pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<EventixState>,
    form: Form,
    req: Request,
) -> Result<impl IntoResponse, HTMLError> {
    let state = state.lock().await;

    let events = Events::new(&state, &locale);
    let tasks = Tasks::new(&state, &locale);

    let html = CollectionAddTemplate {
        page,
        name: form.name.unwrap_or_default(),
        syncer: SyncerTemplate::new(locale.clone(), "syncer", form.syncer, None),
        prev: req.prev.as_ref(),
        locale,
        events,
        tasks,
    }
    .render()
    .context("collections add template")?;

    Ok(Html(html))
}
