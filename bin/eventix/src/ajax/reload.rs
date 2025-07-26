use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use ical::objects::CalDate;
use serde::Serialize;
use std::collections::HashMap;
use tracing::error;

use crate::html;
use crate::locale::{self, TimeFlags};
use crate::pages::error::HTMLError;
use crate::state::EventixState;
use crate::state::SyncResult;

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
    let locale = locale::default();

    let sync_res = match crate::state::State::reload(state).await {
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
