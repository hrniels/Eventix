pub mod index;

use axum::{routing::get, Router};

use crate::state::State;

use super::Page;

pub async fn new_page(state: &State) -> Page {
    Page::new(state).await
}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/", get(self::index::handler))
        .with_state(state)
}
