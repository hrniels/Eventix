mod index;

use axum::{routing::get, Router};
use index::Filter;

use crate::state::State;

use super::{Breadcrumb, Page};

pub async fn new_page(state: &State, req: &Filter) -> Page {
    let mut page = Page::new(state).await;
    page.add_breadcrumb(Breadcrumb::new(req.url(), "List"));
    page
}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/list", get(self::index::handler))
        .with_state(state)
}
