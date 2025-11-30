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
    cal_id: String,
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
    match req.op {
        Operation::Delete => {
            eventix_state::State::delete_calendar(state.clone(), &req.col_id, &req.cal_id)
                .await
                .context(format!(
                    "Unable to reload calendar {}:{}",
                    req.col_id, req.cal_id
                ))?;
        }
        Operation::Toggle => {
            let mut state = state.lock().await;

            let col = state
                .settings_mut()
                .collections_mut()
                .get_mut(&req.col_id)
                .ok_or_else(|| anyhow!("No collection '{}'", &req.col_id))?;

            let cal = col
                .all_calendars_mut()
                .get_mut(&req.cal_id)
                .ok_or_else(|| anyhow!("No calendar '{}'", &req.cal_id))?;
            cal.set_enabled(!cal.enabled());

            if let Err(e) = state.settings().write_to_file() {
                tracing::warn!("Unable to save settings: {}", e);
            }
        }
    }

    eventix_state::State::refresh_store(state).await?;

    Ok(Json(()))
}
