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
        alarm::AlarmTemplate, attendees::AttendeesTemplate, calcombo::CalComboTemplate,
        datetimerange::DateTimeRangeTemplate, recur::RecurTemplate, todostatus::TodoStatusTemplate,
    },
    locale::{self, DateFlags, Locale, TimeFlags},
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
    calendars: CalComboTemplate,
    attendees: AttendeesTemplate,
    status: Option<TodoStatusTemplate>,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let calendar = state.last_calendar().lock().await.get(&req.ctype).cloned();
    content(
        super::new_page(&state, &req).await,
        locale.clone(),
        State(state),
        CompNew::new(&req, locale.timezone(), calendar),
    )
    .await
}

pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<crate::state::State>,
    form: CompNew,
) -> Result<impl IntoResponse, HTMLError> {
    let (store, disabled) = state.acquire_store_and_disabled().await;

    let events = Events::new(&store, &disabled, &locale);
    let tasks = Tasks::new(&store, &disabled, &locale);
    let calendar = Arc::from(form.calendar.clone());

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
        calendars: CalComboTemplate::new(
            "calendar",
            store.sources_for_type(form.req.ctype),
            calendar,
        ),
        attendees: AttendeesTemplate::new(locale.clone(), "attendees", form.attendees),
        status: form
            .status
            .map(|st| TodoStatusTemplate::new(locale.clone(), "status", st)),
        events,
        locale,
        tasks,
        ctype: form.req.ctype,
    }
    .render()
    .context("new template")?;

    Ok(Html(html))
}
