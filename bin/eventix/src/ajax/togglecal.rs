use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{error::HTMLError, state::EventixState};

#[derive(Debug, Deserialize)]
pub struct Request {
    id: String,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/toggle-calendar", post(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let mut state = state.lock().await;

    let settings = state.settings_mut();
    settings.toggle_calendar(&req.id);
    // permanently remember the new calendar state
    if let Err(e) = settings.write_to_file().await {
        warn!("Unable to save settings: {}", e);
    }

    Ok(Json(Response {}))
}
