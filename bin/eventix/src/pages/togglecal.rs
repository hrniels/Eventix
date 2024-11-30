use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::error::HTMLError;

#[derive(Debug, Deserialize)]
pub struct Request {
    id: String,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn path() -> &'static str {
    "/toggle-calendar"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new().route("/", get(handler)).with_state(state)
}

async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let mut disabled = state.disabled_cals().lock().await;
    if disabled.contains(&req.id) {
        disabled.retain(|d| d != &req.id);
    } else {
        disabled.push(req.id);
    }

    Ok(Json(Response {}))
}
