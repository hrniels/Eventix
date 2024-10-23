use anyhow::{anyhow, Context, Result};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use ical::col::{CalSource, Occurrence};
use ical::objects::EventLike;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use ical::objects::CalDate;

use crate::error::HTMLError;
use crate::html::filters;
use crate::locale::{self, Locale};
use crate::pages::Page;

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
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
    source: &'a CalSource,
    occ: Occurrence<'a>,
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(path().to_string());
    let locale = locale::default();

    let rid = req.rid.as_ref().and_then(|rid| rid.parse::<CalDate>().ok());

    let occ = state
        .store()
        .occurrence_by_id(&req.uid, rid.as_ref(), locale.timezone())
        .context(format!(
            "Unable to find occurrence with uid '{}' and rid '{:?}'",
            &req.uid, req.rid
        ))?;
    let source = state.store().source(occ.source()).unwrap();

    let html = EventTemplate {
        page,
        locale,
        source,
        occ,
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
