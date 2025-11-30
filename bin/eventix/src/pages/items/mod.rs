mod add;
mod edit;

use axum::Router;
use eventix_state::EventixState;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .nest("/add", add::router(state.clone()))
        .nest("/edit", edit::router(state.clone()))
}
