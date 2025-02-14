use anyhow::{Context, Result};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use chrono::{Duration, Local};
use ical::objects::{CalCompType, CalTodoStatus, EventLike};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::HTMLError;
use crate::html::{self, filters};
use crate::locale::{self, Locale};

use crate::objects::DayOccurrence;

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
}

#[derive(Debug, Serialize)]
struct Response {
    html: String,
}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/occlist", get(handler))
        .with_state(state)
}

#[derive(Template)]
#[template(path = "pages/occlist.htm")]
struct OccListTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    occs: Vec<DayOccurrence<'a>>,
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let now = Local::now().with_timezone(locale.timezone());
    let start = now - Duration::days(180);
    let end = now + Duration::days(180);

    let store = state.store().lock().await;

    let item = store
        .item_by_id(&req.uid)
        .context(format!("Unable to find item with uid {}", req.uid))?;

    let occs = item.occurrences_within(start, end);
    let occs = occs.map(|ref o| DayOccurrence::new(o)).collect();

    let html = OccListTemplate { occs, locale }
        .render()
        .context("details template")?;

    Ok(Json(Response { html }))
}
