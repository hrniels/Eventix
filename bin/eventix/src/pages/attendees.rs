use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::error::HTMLError;

#[derive(Debug, Deserialize)]
pub struct Request {
    term: String,
}

#[derive(Debug, Serialize)]
struct Response(Vec<String>);

pub fn path() -> &'static str {
    "/attendees"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new().route("/", get(handler)).with_state(state)
}

async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let store = state.store().lock().await;
    let mut contacts = store
        .contacts()
        .iter()
        .filter(|(address, name)| name.contains(&req.term) || address.contains(&req.term))
        .map(|(address, name)| {
            if address == name {
                address.clone()
            } else {
                let address = match address.strip_prefix("mailto:") {
                    Some(addr) => addr,
                    None => address,
                };
                format!("{} <{}>", name, address)
            }
        })
        .collect::<Vec<_>>();
    contacts.sort();
    Ok(Json(Response(contacts)))
}
