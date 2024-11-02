mod index;
mod update;

use axum::{
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

use super::{Breadcrumb, Page};

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
}

pub fn new_page(req: &Request) -> Page {
    let mut page = Page::new(path().to_string());
    page.add_breadcrumb(Breadcrumb::new(
        format!("{}?{}", path(), serde_qs::to_string(req).unwrap()),
        "Edit",
    ));
    page
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
