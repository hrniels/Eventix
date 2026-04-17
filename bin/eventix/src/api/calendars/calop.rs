// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_state::EventixState;
use serde::Deserialize;

use crate::api::JsonError;

#[derive(Debug, Deserialize)]
pub enum Operation {
    Delete,
    Toggle,
}

#[derive(Debug, Deserialize)]
pub struct Params {
    col_id: String,
    cal_id: Option<String>,
    folder: Option<String>,
    op: Operation,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/calop", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Params>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    let mut state = state.lock().await;

    match req.op {
        Operation::Delete => {
            match (&req.cal_id, &req.folder) {
                (Some(cal_id), _) => {
                    eventix_state::State::delete_calendar(&mut state, &req.col_id, cal_id)
                        .await
                        .context(format!(
                            "Unable to delete calendar {}:{}",
                            req.col_id, cal_id
                        ))?;
                }
                (None, Some(folder)) => {
                    eventix_state::State::delete_calendar_by_folder(
                        &mut state,
                        &req.col_id,
                        folder,
                    )
                    .await
                    .context(format!(
                        "Unable to delete remote calendar by folder {}:{}",
                        req.col_id, folder
                    ))?;
                }
                (None, None) => return Err(anyhow!("Missing calendar id or folder").into()),
            }

            if let Err(e) = state.settings().write_to_file() {
                tracing::warn!("Unable to save settings: {}", e);
            }
        }
        Operation::Toggle => {
            let cal_id = req
                .cal_id
                .as_ref()
                .ok_or_else(|| anyhow!("Missing calendar id"))?;
            let col = state
                .settings_mut()
                .collections_mut()
                .get_mut(&req.col_id)
                .ok_or_else(|| anyhow!("No collection '{}'", &req.col_id))?;

            let cal = col
                .all_calendars_mut()
                .get_mut(cal_id)
                .ok_or_else(|| anyhow!("No calendar '{}'", cal_id))?;
            cal.set_enabled(!cal.enabled());

            if let Err(e) = state.settings().write_to_file() {
                tracing::warn!("Unable to save settings: {}", e);
            }
        }
    }

    eventix_state::State::refresh_store(&mut state).await?;

    Ok(Json(()))
}
