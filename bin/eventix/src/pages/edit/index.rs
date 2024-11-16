use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use ical::{
    col::Occurrence,
    objects::{CalDate, EventLike},
};
use std::sync::Arc;

use super::{Page, Request};
use crate::{
    comps::{datetimerange::DateTimeRangeTemplate, recur::RecurTemplate},
    locale::{self, Locale},
};
use crate::{error::HTMLError, pages::tasks::Tasks};
use crate::{html::filters, pages::events::Events};

#[derive(Template)]
#[template(path = "pages/edit.htm")]
struct EditTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    uid: String,
    rid: Option<String>,
    summary: &'a String,
    location: &'a String,
    description: &'a String,
    start_end: DateTimeRangeTemplate<'a>,
    rrule: RecurTemplate<'a>,
    occ: &'a Occurrence<'a>,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    content(
        super::new_page(&req),
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

    let html = EditTemplate {
        page,
        uid: req.uid.clone(),
        rid: req.rid.clone(),
        summary: occ.summary().unwrap_or(&String::from("")),
        location: occ.location().unwrap_or(&String::from("")),
        description: occ.description().unwrap_or(&String::from("")),
        start_end: DateTimeRangeTemplate::new(
            locale.clone(),
            "start_end",
            Some(occ.occurrence_startdate()),
            occ.occurrence_enddate(),
        ),
        rrule: RecurTemplate::new(locale.clone(), "rrule", occ.rrule()),
        occ: &occ,
        events,
        locale,
        tasks,
    }
    .render()
    .context("edit template")?;

    Ok(Html(html))
}
