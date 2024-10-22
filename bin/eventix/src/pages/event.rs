use anyhow::{anyhow, Context, Result};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use ical::objects::CalEvent;

use crate::error::HTMLError;
use crate::html::filters;
use crate::locale::{self, Locale};
use crate::pages::Page;

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
}

#[derive(Debug, Serialize)]
struct Response {
    html: String,
}

#[derive(Template)]
#[template(path = "pages/event.htm")]
struct EventTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    event: &'a CalEvent,
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(path().to_string());
    let locale = locale::default();

    let event = state
        .store()
        .component_by_uid(&req.uid)
        .context(format!("Unable to find component with uid '{}'", &req.uid))?
        .as_event()
        .ok_or_else(|| anyhow!("Component with uid '{}' is no event", &req.uid))?;

    let html = EventTemplate {
        page,
        locale,
        event,
    }
    .render()
    .context("event template")?;

    Ok(Json(Response { html }))
}

pub fn path() -> &'static str {
    "/event"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new().route("/", get(handler)).with_state(state)
}
