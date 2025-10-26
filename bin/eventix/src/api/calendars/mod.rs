pub mod calbox;
pub mod calop;
pub mod savecal;
pub mod syncop;

use axum::Router;
use eventix_state::EventixState;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .with_state(state.clone())
        .merge(calbox::router(state.clone()))
        .merge(calop::router(state.clone()))
        .merge(savecal::router(state.clone()))
        .merge(syncop::router(state.clone()))
}
