use anyhow::{anyhow, Context};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::error::HTMLError;
use crate::state::EventixState;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/delete", get(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(form): Query<Request>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let mut state = state.lock().await;
    let file = state
        .store_mut()
        .files_by_id_mut(&form.uid)
        .ok_or_else(|| anyhow!("Unable to find file with uid {}", form.uid))?;

    let src = file.directory().clone();
    state
        .store_mut()
        .directory_mut(&src)
        .unwrap()
        .delete_by_uid(&form.uid)
        .with_context(|| format!("Unable to delete item with uid {}", form.uid))?;

    Ok(Json(Response {}))
}
