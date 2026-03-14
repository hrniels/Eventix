// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, RawQuery, State},
    response::{Html, IntoResponse},
};
use eventix_ical::objects::{CalCompType, CalDate, CalPartStat, EventLike};
use eventix_locale::{DateFlags, Locale, TimeFlags};
use eventix_state::{CalendarAlarmType, EventixState};
use std::sync::Arc;

use crate::comps::{
    alarm::AlarmTemplate, attendees::AttendeesTemplate, calcombo::CalComboTemplate,
    datetimerange::DateTimeRangeTemplate, recur::RecurTemplate, todostatus::TodoStatusTemplate,
};
use crate::html::filters;
use crate::objects::Calendars;
use crate::pages::{Page, error::HTMLError, events::Events, tasks::Tasks};

use super::{CompNew, Request};

/// Full-page shell template. The form content is loaded separately via AJAX.
#[derive(Template)]
#[template(path = "pages/items/add.htm")]
struct NewTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    /// The raw query string from the request URL, passed through to seed the first AJAX content
    /// request (e.g. `"ctype=Event&prev=%2Fpages%2Flist"`).
    request_query: String,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

/// Fragment-only template for the add-item form, rendered by the AJAX content endpoint.
#[derive(Template)]
#[template(path = "pages/items/add_content.htm")]
struct NewContentTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    prev: Option<&'a String>,
    ctype: CalCompType,
    summary: &'a String,
    location: &'a String,
    description: &'a String,
    start_end: DateTimeRangeTemplate<'a>,
    rrule: RecurTemplate<'a>,
    alarm: AlarmTemplate<'a>,
    calendars: CalComboTemplate,
    cal_personal: Vec<(&'a String, bool)>,
    attendees: AttendeesTemplate,
    status: Option<TodoStatusTemplate>,
}

pub async fn handler(
    State(state): State<EventixState>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, HTMLError> {
    let page = super::new_page(&state).await;

    let st = state.lock().await;
    let locale = st.locale();
    let events = Events::new(&st, &locale);
    let tasks = Tasks::new(&st, &locale);

    let request_query = raw.unwrap_or_default();

    let html = NewTemplate {
        page,
        locale,
        request_query,
        events,
        tasks,
    }
    .render()
    .context("new template")?;

    Ok(Html(html))
}

/// Renders only the add-item form fragment for the given request. Used by both the
/// AJAX content endpoint (GET) and the save handler (POST) to re-render the form.
pub async fn content_fragment(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let (locale, calendar) = {
        let state = state.lock().await;
        let locale = state.locale();
        let calendar = state.misc().last_calendar(req.ctype).cloned();
        (locale, calendar)
    };

    content(
        Page::default(),
        locale.clone(),
        State(state),
        CompNew::new(&req, locale.timezone(), calendar),
        req,
    )
    .await
}

/// Renders the add-item form fragment with the given page state and form data.
/// Called by `content_fragment` for the initial GET and by `save::handler` after a POST.
pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<EventixState>,
    form: CompNew,
    req: Request,
) -> Result<impl IntoResponse, HTMLError> {
    let state = state.lock().await;

    let calendar: Arc<String> = Arc::from(form.calendar.clone());

    let cal_personal = state
        .settings()
        .calendars()
        .map(|(id, settings)| {
            (
                id,
                matches!(settings.alarms(), CalendarAlarmType::Personal { .. }),
            )
        })
        .collect();

    let html = NewContentTemplate {
        page,
        summary: &form.summary,
        location: &form.location,
        description: &form.description,
        start_end: DateTimeRangeTemplate::new(
            locale.clone(),
            req.ctype,
            "start_end",
            Some(form.start_end),
        ),
        rrule: RecurTemplate::new(locale.clone(), "rrule", form.rrule),
        alarm: AlarmTemplate::new(locale.clone(), "alarm", false, true, None, form.alarm),
        calendars: CalComboTemplate::new(
            "calendar",
            Calendars::new(&state, |_id, settings| {
                settings.types().contains(&req.ctype)
            }),
            calendar.clone(),
            false,
        ),
        cal_personal,
        attendees: AttendeesTemplate::new(
            locale.clone(),
            "attendees",
            state.settings().emails(),
            Some(String::from("calendar")),
            form.attendees,
        ),
        status: form
            .status
            .map(|st| TodoStatusTemplate::new(locale.clone(), "status", st)),
        locale,
        ctype: req.ctype,
        prev: req.prev.as_ref(),
    }
    .render()
    .context("new content template")?;

    Ok(Html(html))
}
