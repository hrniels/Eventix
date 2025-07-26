mod index;

use axum::{Router, routing::get};
use eventix_state::EventixState;
use index::Filter;

use super::{Breadcrumb, Page};

pub async fn new_page(state: &EventixState, req: &Filter) -> Page {
    let mut page = Page::new(state).await;
    page.add_breadcrumb(Breadcrumb::new(req.url(), "List"));
    page
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/list", get(self::index::handler))
        .with_state(state)
}
