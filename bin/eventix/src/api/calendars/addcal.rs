use anyhow::anyhow;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_state::{CalendarSettings, EventixState};
use serde::Deserialize;

use crate::api::JsonError;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/addcal", post(handler))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct Params {
    col_id: String,
    folder: String,
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Params>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    let mut state = state.lock().await;

    let col = state
        .settings_mut()
        .collections_mut()
        .get_mut(&req.col_id)
        .ok_or_else(|| anyhow!("No collection '{}'", &req.col_id))?;

    let id = uuid::Uuid::new_v4().simple().to_string();
    let mut cal = CalendarSettings::default();
    cal.set_folder(req.folder.clone());
    cal.set_name(req.folder);
    cal.set_bgcolor("#555555".to_string());
    cal.set_fgcolor("#ffffff".to_string());
    col.all_calendars_mut().insert(id, cal);

    if let Err(e) = state.settings().write_to_file() {
        tracing::warn!("Unable to save settings: {}", e);
    }

    eventix_state::State::refresh_store(&mut state).await?;

    Ok(Json(()))
}
