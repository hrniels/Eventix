mod index;
mod update;

use axum::{
    routing::{get, post},
    Router,
};

use super::Page;

pub fn new_page() -> Page {
    Page::new(path().to_string())
}

pub fn path() -> &'static str {
    "/edit"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/", get(self::index::handler))
        .route("/update", post(self::update::handler))
        .with_state(state)
}
