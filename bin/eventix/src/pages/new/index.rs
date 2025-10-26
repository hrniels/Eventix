use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
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

#[derive(Template)]
#[template(path = "pages/new.htm")]
struct NewTemplate<'a> {
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
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let (locale, calendar) = {
        let state = state.lock().await;
        let locale = state.settings().locale();
        let calendar = state.misc().last_calendar(req.ctype).cloned();
        (locale, calendar)
    };

    content(
        super::new_page(&state).await,
        locale.clone(),
        State(state),
        CompNew::new(&req, locale.timezone(), calendar),
        req,
    )
    .await
}

pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<EventixState>,
    form: CompNew,
    req: Request,
) -> Result<impl IntoResponse, HTMLError> {
    let state = state.lock().await;

    let events = Events::new(&state, &locale);
    let tasks = Tasks::new(&state, &locale);
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

    let html = NewTemplate {
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
            Calendars::new(&state, |settings| settings.types().contains(&req.ctype)),
            calendar.clone(),
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
        events,
        locale,
        tasks,
        ctype: req.ctype,
        prev: req.prev.as_ref(),
    }
    .render()
    .context("new template")?;

    Ok(Html(html))
}
