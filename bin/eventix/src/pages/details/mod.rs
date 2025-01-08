mod index;

use axum::{routing::get, Router};

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/details", get(self::index::handler))
        .with_state(state)
}
