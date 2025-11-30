use axum::{
    Json, Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::post,
};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::api::JsonError;

#[derive(Debug, Deserialize)]
pub struct Request {
    id: String,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/togglecal", post(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, JsonError> {
    let mut state = state.lock().await;

    let misc = state.misc_mut();
    misc.toggle_calendar(&req.id);
    // permanently remember the new calendar state
    if let Err(e) = misc.write_to_file() {
        warn!("Unable to misc state: {}", e);
    }

    Ok(Json(Response {}))
}
