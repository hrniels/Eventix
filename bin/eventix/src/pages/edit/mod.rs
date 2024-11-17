mod delete;
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
    let name = if req.rid.is_some() {
        "Edit occurrence"
    } else {
        "Edit series"
    };
    page.add_breadcrumb(Breadcrumb::new(
        format!("{}?{}", path(), serde_qs::to_string(req).unwrap()),
        name,
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
        .route("/delete", get(self::delete::handler))
        .with_state(state)
}
