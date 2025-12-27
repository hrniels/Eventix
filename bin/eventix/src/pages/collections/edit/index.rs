use anyhow::{Context, Result, anyhow};
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
#[template(path = "pages/collections/edit.htm")]
struct CollectionAddTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    col_id: &'a String,
    prev: Option<&'a String>,
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
        None,
        req,
    )
    .await
}

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

    let events = Events::new(&state, &locale);
    let tasks = Tasks::new(&state, &locale);
    let syncer = form.syncer_type();

    let html = CollectionAddTemplate {
        page,
        syncer: SyncerTemplate::new(locale.clone(), "syncer", form.syncer, syncer),
        col_id: &req.col_id,
        prev: req.prev.as_ref(),
        locale,
        events,
        tasks,
    }
    .render()
    .context("collections edit template")?;

    Ok(Html(html))
}
