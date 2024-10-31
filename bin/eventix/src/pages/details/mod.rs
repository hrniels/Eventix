mod index;

use axum::{routing::get, Router};

pub fn path() -> &'static str {
    "/details"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/", get(self::index::handler))
        .with_state(state)
}
