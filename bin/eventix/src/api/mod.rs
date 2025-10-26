pub mod attendees;
pub mod auth;
pub mod calendars;
pub mod help;
pub mod items;
pub mod togglecal;

use axum::Router;
use eventix_state::EventixState;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .with_state(state.clone())
        .nest("/calendars", calendars::router(state.clone()))
        .nest("/items", items::router(state.clone()))
        .merge(attendees::router(state.clone()))
        .merge(auth::router(state.clone()))
        .merge(help::router(state.clone()))
        .merge(togglecal::router(state.clone()))
}
