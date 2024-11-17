pub mod index;

use axum::{routing::get, Router};

use super::Page;

pub fn new_page() -> Page {
    Page::new(path().to_string())
}

pub fn path() -> &'static str {
    "/"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/", get(self::index::handler))
        .with_state(state)
}
