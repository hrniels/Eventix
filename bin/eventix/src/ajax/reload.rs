use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use eventix_ical::objects::CalDate;
use eventix_locale::TimeFlags;
use eventix_state::{EventixState, SyncResult};
use serde::Serialize;
use std::collections::HashMap;
use tracing::error;

use crate::html;
use crate::pages::error::HTMLError;

#[derive(Debug, Serialize)]
struct Response {
    changed: bool,
    calendars: HashMap<String, bool>,
    date: String,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/reload", post(handler))
        .with_state(state)
}

async fn handler(State(state): State<EventixState>) -> Result<impl IntoResponse, HTMLError> {
    let locale = eventix_locale::default();

    let sync_res = match eventix_state::State::reload(state).await {
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
