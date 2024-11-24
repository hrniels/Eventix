use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use std::sync::Arc;

use ical::objects::{CalCompType, CalDate, EventLike};

use super::{CompNew, Page, Request};
use crate::{
    comps::{
        alarm::AlarmTemplate, calcombo::CalComboTemplate, datetimerange::DateTimeRangeTemplate,
        recur::RecurTemplate,
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
    ctype: CalCompType,
    summary: &'a String,
    location: &'a String,
    description: &'a String,
    start_end: DateTimeRangeTemplate<'a>,
    rrule: RecurTemplate<'a>,
    reminder: AlarmTemplate<'a>,
    calendars: CalComboTemplate<'a>,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    content(
        super::new_page(&req),
        locale.clone(),
        State(state),
        CompNew::new(req.ctype, locale.timezone()),
    )
    .await
}

pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<crate::state::State>,
    form: CompNew,
) -> Result<impl IntoResponse, HTMLError> {
    let store = state.store().lock().unwrap();

    let events = Events::new(&store, &locale);
    let tasks = Tasks::new(&store, &locale);

    let html = NewTemplate {
        page,
        summary: &form.summary,
        location: &form.location,
        description: &form.description,
        start_end: DateTimeRangeTemplate::new(
            locale.clone(),
            form.req.ctype,
            "start_end",
            Some(form.start_end),
        ),
        rrule: RecurTemplate::new(locale.clone(), "rrule", form.rrule),
        reminder: AlarmTemplate::new(locale.clone(), "reminder", form.reminder),
        calendars: CalComboTemplate::new("calendar", store.sources(), form.calendar),
        events,
        locale,
        tasks,
        ctype: form.req.ctype,
    }
    .render()
    .context("new template")?;

    Ok(Html(html))
}
