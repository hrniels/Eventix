use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
use ical::objects::CalDate;
use serde::Serialize;
use tracing::error;

use crate::{
    error::HTMLError,
    html,
    locale::{self, TimeFlags},
    state::EventixState,
};

#[derive(Debug, Serialize)]
struct Response {
    changed: bool,
    date: String,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/reload", post(handler))
        .with_state(state)
}

async fn handler(State(state): State<EventixState>) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();

    let mut state = state.lock().await;
    let changed = match state.reload().await {
        Err(e) => {
            error!("Unable to reload state: {}", e);
            false
        }
        Ok(changed) => changed,
    };

    Ok(Json(Response {
        changed,
        date: html::filters::time(
            &CalDate::now().as_start_with_tz(locale.timezone()),
            &locale,
            TimeFlags::None,
        )
        .unwrap(),
    }))
}
