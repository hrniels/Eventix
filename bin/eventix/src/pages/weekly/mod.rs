pub mod index;

use axum::{Router, routing::get};
use eventix_state::EventixState;

use super::Page;

pub async fn new_page(state: &EventixState) -> Page {
    Page::new(state).await
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/weekly", get(self::index::handler))
        .with_state(state)
}
