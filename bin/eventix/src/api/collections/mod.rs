pub mod delete;

use axum::Router;
use eventix_state::EventixState;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .with_state(state.clone())
        .merge(delete::router(state.clone()))
}
