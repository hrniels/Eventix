use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{error::HTMLError, settings::Settings, state::EventixState};

#[derive(Debug, Deserialize)]
pub struct Request {
    id: String,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/toggle-calendar", get(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let mut state = state.lock().await;

    state.toggle_calendar(&req.id);

    // permanently remember the new calendar state
    let settings = Settings::new_from_state(&state).await;
    if let Err(e) = settings.write_to_file().await {
        warn!("Unable to save settings: {}", e);
    }

    Ok(Json(Response {}))
}
