use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;
use tracing::error;

use crate::error::HTMLError;

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/reload", get(handler))
        .with_state(state)
}

async fn handler(State(state): State<crate::state::State>) -> Result<impl IntoResponse, HTMLError> {
    if let Err(e) = state.reload().await {
        error!("Unable to reload state: {}", e);
    }

    Ok(Json(Response {}))
}
