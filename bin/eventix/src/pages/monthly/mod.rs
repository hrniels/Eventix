pub mod index;

use axum::{Router, routing::get};

use crate::state::EventixState;

use super::Page;

pub async fn new_page(state: &EventixState) -> Page {
    Page::new(state).await
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/", get(self::index::handler))
        .with_state(state)
}
