use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use std::sync::Arc;

use ical::objects::{CalDate, EventLike};

use super::{Page, Save};
use crate::{
    comps::{
        calcombo::CalComboTemplate, datetimerange::DateTimeRangeTemplate, recur::RecurTemplate,
    },
    locale::{self, Locale},
};
use crate::{error::HTMLError, pages::tasks::Tasks};
use crate::{html::filters, pages::events::Events};

#[derive(Template)]
#[template(path = "pages/new.htm")]
struct NewTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    summary: &'a String,
    location: &'a String,
    description: &'a String,
    start_end: DateTimeRangeTemplate<'a>,
    rrule: RecurTemplate<'a>,
    calendars: CalComboTemplate<'a>,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<crate::state::State>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    content(
        super::new_page(),
        locale.clone(),
        State(state),
        Save::new(locale.timezone()),
    )
    .await
}

pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<crate::state::State>,
    form: Save,
) -> Result<impl IntoResponse, HTMLError> {
    let store = state.store().lock().unwrap();

    let events = Events::new(&store, &locale, 7);
    let tasks = Tasks::new(&store, &locale, 7);

    let html = NewTemplate {
        page,
        summary: &form.summary,
        location: &form.location,
        description: &form.description,
        start_end: DateTimeRangeTemplate::new(locale.clone(), "start_end", Some(form.start_end)),
        rrule: RecurTemplate::new(locale.clone(), "rrule", form.rrule),
        calendars: CalComboTemplate::new(
            locale.clone(),
            "calendar",
            store.sources(),
            form.calendar,
        ),
        events,
        locale,
        tasks,
    }
    .render()
    .context("new template")?;

    Ok(Html(html))
}
