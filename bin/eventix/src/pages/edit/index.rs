use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use ical::{
    col::{CalDir, Occurrence},
    objects::{CalDate, EventLike},
};
use std::sync::Arc;

use super::{CompEdit, Page, Request};
use crate::{
    comps::{
        alarm::AlarmTemplate, attendees::AttendeesTemplate, calcombo::CalComboTemplate,
        datetimerange::DateTimeRangeTemplate, recur::RecurTemplate, todostatus::TodoStatusTemplate,
    },
    locale::{self, DateFlags, Locale, TimeFlags},
    objects::Calendars,
    pages::Breadcrumb,
    state::EventixState,
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
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    content(
        super::new_page(&state).await,
        locale::default(),
        State(state),
        Query(req),
        None,
    )
    .await
}

pub async fn content(
    mut page: Page,
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
                .context(format!("Invalid rid date: {}", rid))?,
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

    let pers_calendar = state.personal_alarms().get(&*file.directory());
    let pers_alarms = pers_calendar
        .and_then(|cal_alarms| cal_alarms.get(&req.uid, rid.as_ref()))
        .map(|pers_alarms| pers_alarms.alarms());
    let effective_alarms = state.personal_alarms().effective_alarms(&occ);

    page.add_breadcrumb(Breadcrumb::new(
        format!("/edit?{}", serde_qs::to_string(&req).unwrap()),
        super::build_title(&occ, &req.rid),
    ));

    let form = match form {
        Some(f) => f,
        None => {
            let cal = if req.rid.is_none() {
                Some((*file.directory()).to_string())
            } else {
                None
            };
            CompEdit::new_from_occurrence(req, &occ, pers_alarms, cal, locale.timezone())
        }
    };

    let dir = state.store().directory(file.directory()).unwrap();

    let events = Events::new(&state, &locale);
    let tasks = Tasks::new(&state, &locale);

    let html = EditTemplate {
        page,
        uid: form.req.uid.clone(),
        rid: form.req.rid.clone(),
        dir,
        calendars: form.calendar.map(|cal| {
            CalComboTemplate::new(
                "calendar",
                Calendars::new(&state, |_dir, settings| {
                    settings.types().contains(&occ.ctype()) && !settings.disabled()
                }),
                Arc::new(cal.to_string()),
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
            effective_alarms.as_ref(),
            form.alarm,
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
        events,
        locale,
        tasks,
    }
    .render()
    .context("edit template")?;

    Ok(Html(html))
}
