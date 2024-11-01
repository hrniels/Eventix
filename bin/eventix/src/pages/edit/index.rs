use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use chrono::{NaiveDate, Utc};
use ical::col::CalStore;
use ical::objects::{CalDate, EventLike};
use serde::Deserialize;
use std::sync::{Arc, MutexGuard};

use super::Page;
use crate::locale::{self, Locale};
use crate::{error::HTMLError, pages::tasks::Tasks};
use crate::{html::filters, pages::events::Events};

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
}

impl Request {
    pub fn new(uid: String, rid: Option<String>) -> Self {
        Self { uid, rid }
    }
}

#[derive(Template)]
#[template(path = "pages/edit.htm")]
struct OverviewTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    uid: String,
    rid: Option<String>,
    summary: &'a String,
    store: &'a MutexGuard<'a, CalStore>,
    today: NaiveDate,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    content(
        super::new_page(),
        locale::default(),
        State(state),
        Query(req),
    )
    .await
}

pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let timezone = *locale.timezone();

    let store = state.store().lock().unwrap();

    let item = store
        .item_by_id(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    let rid = if let Some(ref rid) = req.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {}", rid))?,
        )
    } else {
        None
    };

    let occ = item
        .occurrence_by_id(&req.uid, rid.as_ref(), locale.timezone())
        .context(format!(
            "Unable to find occurrence with uid '{}' and rid '{:?}'",
            &req.uid, rid
        ))?;

    let events = Events::new(&store, &locale, 7);
    let tasks = Tasks::new(&store, &locale, 7);

    let html = OverviewTemplate {
        page,
        locale,
        uid: req.uid.clone(),
        rid: req.rid.clone(),
        summary: occ.summary().unwrap_or(&String::from("")),
        store: &store,
        today: Utc::now().with_timezone(&timezone).date_naive(),
        events,
        tasks,
    }
    .render()
    .context("edit template")?;

    Ok(Html(html))
}
