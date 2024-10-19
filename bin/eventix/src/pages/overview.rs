use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use std::sync::Arc;

use super::Page;
use crate::error::HTMLError;
use crate::html::filters;
use crate::locale::{self, Locale};

#[derive(Template)]
#[template(path = "pages/overview.htm")]
struct OverviewTemplate {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
}

async fn handler(
    State(_state): State<crate::state::State>,
) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(path().to_string());
    let locale = locale::default();

    let html = OverviewTemplate { page, locale }
        .render()
        .context("overview template")?;

    Ok(Html(html))
}

pub fn path() -> &'static str {
    "/"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new().route("/", get(handler)).with_state(state)
}
