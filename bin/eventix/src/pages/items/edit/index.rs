// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result, anyhow};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use eventix_ical::{
    col::{CalDir, Occurrence},
    objects::{CalDate, EventLike},
};
use eventix_locale::Locale;
use eventix_state::{CalendarAlarmType, EventixState};
use std::sync::Arc;

use super::{CompEdit, Request};
use crate::html::filters;
use crate::objects::Calendars;
use crate::pages::{Page, error::HTMLError};
use crate::util;
use crate::{
    comps::{
        alarm::AlarmTemplate, attendees::AttendeesTemplate, calcombo::CalComboTemplate,
        datetimerange::DateTimeRangeTemplate, recur::RecurTemplate, todostatus::TodoStatusTemplate,
    },
    pages::items::edit::EditMode,
};

/// Fragment-only template for the edit-item form, rendered by the AJAX content endpoint.
#[derive(Template)]
#[template(path = "pages/items/edit.htm")]
struct EditTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    edit_start: String,
    prev: &'a String,
    uid: String,
    rid: Option<String>,
    mode: EditMode,
    dir: &'a CalDir,
    calendars: Option<CalComboTemplate>,
    summary: &'a String,
    location: &'a String,
    description: &'a String,
    start_end: DateTimeRangeTemplate<'a>,
    rrule: Option<RecurTemplate<'a>>,
    alarm: AlarmTemplate<'a>,
    attendees: AttendeesTemplate,
    status: Option<TodoStatusTemplate>,
    occ: &'a Occurrence<'a>,
}

/// Renders only the edit-item form fragment for the given request. Used by the AJAX content
/// endpoint (GET).
pub async fn content(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = state.lock().await.locale();
    content_with(Page::default(), locale, State(state), Query(req), None).await
}

/// Renders the edit-item form fragment with the given page state and form data.
/// Called by `content` for the initial GET and by `update::handler` after a POST.
pub async fn content_with(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<EventixState>,
    Query(req): Query<Request>,
    form: Option<CompEdit>,
) -> Result<impl IntoResponse, HTMLError> {
    let state = state.lock().await;

    let file = state
        .store()
        .file_by_id(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    let rid = if let Some(ref rid) = req.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {rid}"))?,
        )
    } else {
        None
    };

    let occ = file
        .occurrence_by_id(&req.uid, rid.as_ref(), locale.timezone())
        .context(format!(
            "Unable to find occurrence with uid '{}' and rid '{:?}'",
            &req.uid, rid
        ))?;

    if !util::user_is_event_owner(file.directory(), &state, &occ) {
        return Err(anyhow!("No edit permission").into());
    }

    let alarm_type = state
        .settings()
        .calendar(file.directory())
        .unwrap()
        .1
        .alarms();
    let pers_calendar = state.personal_alarms().get(file.directory());
    let pers_alarms = pers_calendar
        .and_then(|cal_alarms| cal_alarms.get(&req.uid, rid.as_ref()))
        .map(|pers_alarms| pers_alarms.alarms());
    let effective_alarms = state.personal_alarms().effective_alarms(&occ, alarm_type);

    let form = match form {
        Some(f) => f,
        None => {
            let cal = if req.mode != EditMode::Occurrence {
                Some((*file.directory()).to_string())
            } else {
                None
            };
            CompEdit::new_from_occurrence(&req, &occ, pers_alarms, cal, locale.timezone())
        }
    };

    let dir = state.store().directory(file.directory()).unwrap();
    let have_personal = matches!(
        state.settings().calendar(dir.id()).unwrap().1.alarms(),
        CalendarAlarmType::Personal { .. }
    );

    let alarm_has_start = form.start_end.has_start();
    let alarm_has_end = form.start_end.has_end();
    let html = EditTemplate {
        page,
        prev: &req.prev,
        edit_start: format!("{}", form.edit_start),
        uid: req.uid.clone(),
        rid: req.rid.clone(),
        mode: req.mode,
        dir,
        calendars: form.calendar.map(|cal| {
            CalComboTemplate::new(
                "calendar",
                Calendars::new(&state, |id, settings| {
                    id == &cal || settings.types().contains(&occ.ctype())
                }),
                Arc::new(cal.to_string()),
                false,
            )
        }),
        summary: &form.summary,
        location: &form.location,
        description: &form.description,
        start_end: DateTimeRangeTemplate::new(
            locale.clone(),
            occ.ctype(),
            "start_end",
            Some(form.start_end),
        ),
        rrule: form
            .rrule
            .map(|rr| RecurTemplate::new(locale.clone(), "rrule", rr)),
        alarm: AlarmTemplate::new(
            locale.clone(),
            "alarm",
            true,
            have_personal,
            effective_alarms.as_ref(),
            form.alarm,
            (alarm_has_start, alarm_has_end),
        ),
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
        occ: &occ,
        locale,
    }
    .render()
    .context("edit content template")?;

    Ok(Html(html))
}
