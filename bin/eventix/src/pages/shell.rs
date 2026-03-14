// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::response::{Html, IntoResponse};
use eventix_ical::objects::{CalDate, CalPartStat, EventLike};
use eventix_locale::{DateFlags, Locale, TimeFlags};
use eventix_state::EventixState;
use std::sync::Arc;

use crate::{
    html::filters,
    pages::{Page, error::HTMLError, events::Events, tasks::Tasks},
};

/// Generic full-page shell template.
///
/// Renders the outer page frame (nav, sidebar, etc.) with a single placeholder `<div>` whose
/// content is fetched via AJAX. All per-page shell pages use this template, parameterised by
/// `slug` and `init_query`. The placeholder div always has `id="page-content"`.
#[derive(Template)]
#[template(path = "pages/shell.htm")]
struct ShellTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    events: Events<'a>,
    tasks: Tasks<'a>,
    /// The `data-page-slug` value used by the JS bootstrap to build the AJAX content URL.
    slug: &'a str,
    /// The raw query string forwarded as `data-init-query` to seed the first content request.
    init_query: String,
}

/// Renders the generic full-page shell for the given page slug.
///
/// The raw query string is passed through unchanged as `data-init-query` so the JS bootstrap
/// can seed the first AJAX content request with the same parameters the user navigated to.
pub async fn handler(
    state: EventixState,
    raw: Option<String>,
    slug: &str,
) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(&state).await;

    let st = state.lock().await;

    let locale = st.locale();
    let events = Events::new(&st, &locale);
    let tasks = Tasks::new(&st, &locale);

    let html = ShellTemplate {
        page,
        locale,
        events,
        tasks,
        slug,
        init_query: raw.unwrap_or_default(),
    }
    .render()
    .context("shell template")?;

    Ok(Html(html))
}
