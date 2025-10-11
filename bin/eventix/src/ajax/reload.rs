use axum::extract::Query;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use eventix_ical::objects::CalDate;
use eventix_locale::TimeFlags;
use eventix_state::{EventixState, SyncCalResult, SyncResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::error;

use crate::html;
use crate::pages::error::HTMLError;

#[derive(Debug, Deserialize)]
struct Request {
    auth_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct Response {
    changed: bool,
    calendars: HashMap<String, SyncCalResult>,
    date: String,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/reload", post(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = state.lock().await.settings().locale();

    let sync_res = match eventix_state::State::reload(state, req.auth_url.as_ref()).await {
        Err(e) => {
            error!("Unable to reload state: {}", e);
            SyncResult::default()
        }
        Ok(res) => res,
    };

    Ok(Json(Response {
        changed: sync_res.changed,
        calendars: sync_res.calendars,
        date: html::filters::time(
            &CalDate::now().as_start_with_tz(locale.timezone()),
            &(),
            &locale,
            TimeFlags::None,
        )
        .unwrap(),
    }))
}
