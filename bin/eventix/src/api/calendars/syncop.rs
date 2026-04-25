// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Context;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use eventix_ical::objects::{CalDate, DateContext};
use eventix_locale::TimeFlags;
use eventix_state::{EventixState, State as EventixAppState, SyncColResult, SyncResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::api::JsonError;
use crate::extract::MultiQuery;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Operation {
    SyncAll,
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
    collections: HashMap<String, SyncColResult>,
    calendars: HashMap<String, bool>,
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
    // use tokio::spawn here manually instead of via run_post, because we don't want to hold the
    // state lock during this operation
    let sync_res = tokio::spawn(run_syncop(state.clone(), req))
        .await
        .map_err(anyhow::Error::from)??;

    // loaded afterwards to make cancellation tests work (otherwise they might stop us in this
    // await and we therefore won't even start the operation)
    let locale = state.lock().await.locale();

    Ok(Json(Response {
        changed: sync_res.changed,
        collections: sync_res.collections,
        calendars: sync_res.calendars,
        date: locale.fmt_time(
            &DateContext::system()
                .date(&CalDate::now())
                .start_in(locale.timezone()),
            TimeFlags::None,
        ),
    }))
}

async fn run_syncop(state: EventixState, req: Request) -> anyhow::Result<SyncResult> {
    let sync_res = match req.op {
        Operation::SyncAll => EventixAppState::sync_all(&state, req.auth_url.as_ref())
            .await
            .context("Unable to reload state"),
        Operation::ReloadCollection { col_id } => {
            EventixAppState::reload_collection(&state, &col_id, req.auth_url.as_ref())
                .await
                .context(format!("Unable to reload collection {}", col_id))
        }
        Operation::ReloadCalendar { col_id, cal_id } => {
            EventixAppState::reload_calendar(&state, &col_id, &cal_id, req.auth_url.as_ref())
                .await
                .context(format!("Unable to reload calendar {}:{}", col_id, cal_id))
        }
        Operation::SyncCollection { col_id } => {
            EventixAppState::sync_collection(&state, &col_id, req.auth_url.as_ref())
                .await
                .context(format!("Unable to sync collection {}", col_id))
        }
        Operation::DiscoverCollection { col_id } => {
            EventixAppState::discover_collection(&state, &col_id, req.auth_url.as_ref())
                .await
                .context(format!("Unable to discover collection {}", col_id))
        }
    }?;
    Ok(sync_res)
}
