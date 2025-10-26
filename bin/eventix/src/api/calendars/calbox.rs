use anyhow::{Context, anyhow};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::comps::calbox::{CalendarBox, CalendarBoxMode, CalendarBoxTemplate};
use crate::pages::error::HTMLError;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    col_id: String,
    cal_id: String,
    folder: Option<String>,
    name: Option<String>,
    color: Option<String>,
    mode: CalendarBoxMode,
}

#[derive(Debug, Serialize)]
struct Response {
    html: String,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/calbox", get(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let state = state.lock().await;

    let locale = state.settings().locale();
    let col = state
        .settings()
        .collections()
        .get(&req.col_id)
        .ok_or_else(|| anyhow!("No collection with id {}", req.col_id))?;

    let cal = col
        .all_calendars()
        .iter()
        .find(|(id, _)| **id == req.cal_id);
    let calbox = match cal {
        Some((id, settings)) => CalendarBox::Known { id: id, settings },
        None => CalendarBox::Unknown {
            id: req.cal_id.clone(),
            folder: req.folder.unwrap_or(String::from("")),
            name: req.name.unwrap_or(String::from("")),
            color: req.color.unwrap_or(String::from("gray")),
        },
    };

    let html = CalendarBoxTemplate::new(
        state.xdg(),
        locale.clone(),
        &req.col_id,
        col,
        calbox,
        req.mode,
    )
    .render()
    .context("auth template")?;

    Ok(Json(Response { html }))
}
