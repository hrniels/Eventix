use anyhow::Context;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use eventix_ical::objects::CalDate;
use eventix_locale::TimeFlags;
use eventix_state::{EventixState, SyncCalResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::api::JsonError;
use crate::extract::MultiQuery;
use crate::html;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Operation {
    ReloadAll,
    ReloadCollection { col_id: String },
    ReloadCalendar { col_id: String, cal_id: String },
    SyncCollection { col_id: String },
    DiscoverCollection { col_id: String },
}

#[derive(Debug, Deserialize)]
struct Request {
    op: Operation,
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
        .route("/syncop", post(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    MultiQuery(req): MultiQuery<Request>,
) -> Result<impl IntoResponse, JsonError> {
    let mut state = state.lock().await;

    let locale = state.settings().locale();

    let sync_res = match req.op {
        Operation::ReloadAll => eventix_state::State::reload(&mut state, req.auth_url.as_ref())
            .await
            .context("Unable to reload state")?,
        Operation::ReloadCollection { col_id } => {
            eventix_state::State::reload_collection(&mut state, &col_id, req.auth_url.as_ref())
                .await
                .context(format!("Unable to reload collection {}", col_id))?
        }
        Operation::ReloadCalendar { col_id, cal_id } => eventix_state::State::reload_calendar(
            &mut state,
            &col_id,
            &cal_id,
            req.auth_url.as_ref(),
        )
        .await
        .context(format!("Unable to reload calendar {}:{}", col_id, cal_id))?,
        Operation::SyncCollection { col_id } => {
            eventix_state::State::sync_collection(&mut state, &col_id, req.auth_url.as_ref())
                .await
                .context(format!("Unable to sync collection {}", col_id))?
        }
        Operation::DiscoverCollection { col_id } => {
            eventix_state::State::discover_collection(&mut state, &col_id, req.auth_url.as_ref())
                .await
                .context(format!("Unable to discover collection {}", col_id))?
        }
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
