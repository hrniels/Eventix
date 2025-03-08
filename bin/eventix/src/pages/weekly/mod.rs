pub mod index;

use axum::{routing::get, Router};

use crate::state::EventixState;

use super::Page;

pub async fn new_page(state: &EventixState) -> Page {
    Page::new(state).await
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/weekly", get(self::index::handler))
        .with_state(state)
}
