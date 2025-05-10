use anyhow::{Context, Result};
use askama::Template;
use axum::Router;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use chrono::offset::LocalResult;
use chrono::{DateTime, Duration, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use ical::col::Occurrence;
use ical::objects::{CalCompType, CalDate, CalTodoStatus, EventLike};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::sync::Arc;

use crate::comps::partstat::PartStatTemplate;
use crate::error::HTMLError;
use crate::html::{self, filters};
use crate::locale::{self, Locale};

use crate::objects::DayOccurrence;
use crate::state::{CalendarAlarmType, EventixState, PersonalAlarms};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
enum Direction {
    Backwards,
    Forward,
}

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    date: String,
    dir: Direction,
    count: usize,
}

#[derive(Debug, Serialize)]
struct Response {
    html: String,
    date: Option<String>,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/occlist", get(handler))
        .with_state(state)
}

struct ListOccurrence<'a> {
    occ: DayOccurrence<'a>,
    partstat: Option<PartStatTemplate>,
    owner: bool,
}

impl<'a> ListOccurrence<'a> {
    pub fn new(
        occ: &'a Occurrence<'a>,
        locale: Arc<dyn Locale + Send + Sync>,
        alarm_type: &CalendarAlarmType,
        pers_alarms: &PersonalAlarms,
        user_mail: Option<&String>,
    ) -> Self {
        let occ = DayOccurrence::new(occ, pers_alarms.has_alarms(occ, alarm_type));

        let owner = occ.is_owned_by(user_mail);
        let partstat = match (user_mail, owner) {
            (Some(user_mail), false) => occ.attendee_status(user_mail).map(|stat| {
                PartStatTemplate::new(
                    locale,
                    format!("occ-{}-{}", occ.uid(), occ.rid_html()),
                    stat,
                    occ.uid().clone(),
                    Some(occ.rid_html()),
                    false,
                )
            }),
            _ => None,
        };

        Self {
            occ,
            partstat,
            owner,
        }
    }
}

impl<'a> Deref for ListOccurrence<'a> {
    type Target = DayOccurrence<'a>;

    fn deref(&self) -> &Self::Target {
        &self.occ
    }
}

#[derive(Template)]
#[template(path = "ajax/occlist.htm")]
struct OccListTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    occs: Vec<ListOccurrence<'a>>,
    personal_alarms: bool,
}

fn min_datetime(timezone: Tz) -> DateTime<Tz> {
    let mut naive = NaiveDateTime::MIN;
    loop {
        match timezone.from_local_datetime(&naive) {
            LocalResult::Single(date) => break date,
            _ => naive += Duration::days(1),
        }
    }
}

fn max_datetime(timezone: Tz) -> DateTime<Tz> {
    let mut naive = NaiveDateTime::MAX;
    loop {
        match timezone.from_local_datetime(&naive) {
            LocalResult::Single(date) => break date,
            _ => naive -= Duration::days(1),
        }
    }
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();

    let date = req
        .date
        .parse::<CalDate>()
        .context(format!("Invalid date: {}", req.date))?
        .as_start_with_tz(locale.timezone());

    let state = state.lock().await;

    let file = state
        .store()
        .file_by_id(&req.uid)
        .context(format!("Unable to find file with uid {}", req.uid))?;

    let occs: Vec<_> = match req.dir {
        Direction::Forward => {
            let start = date + Duration::seconds(1);
            let end = max_datetime(*locale.timezone());
            file.occurrences_between(start, end, |_| true)
                .take(req.count + 1)
                .collect()
        }
        Direction::Backwards => {
            let start = min_datetime(*locale.timezone());
            let end = date;
            let occs = file
                .occurrences_between(start, end, |_| true)
                // ignore the occurrences where the end is later, because we'll find these when
                // walking forward
                .filter(|o| o.occurrence_end().unwrap() < end)
                .collect::<Vec<_>>();
            occs[occs.len().saturating_sub(req.count + 1)..].to_vec()
        }
    };

    let cal_settings = state.settings().calendar(file.directory()).unwrap();
    let alarm_type = cal_settings.alarms();
    let pers_alarms = state.personal_alarms();
    let user_mail = cal_settings.email().map(|e| e.address());

    let more = occs.len() > req.count;
    let occs: Vec<_> = match req.dir {
        Direction::Forward => occs
            .iter()
            .take(req.count)
            .map(|o| ListOccurrence::new(o, locale.clone(), alarm_type, pers_alarms, user_mail))
            .collect(),
        Direction::Backwards => occs
            .iter()
            .skip(if more { 1 } else { 0 })
            .map(|o| ListOccurrence::new(o, locale.clone(), alarm_type, pers_alarms, user_mail))
            .collect(),
    };

    let date = if more {
        match req.dir {
            Direction::Forward => occs.iter().last().and_then(|l| l.occurrence_end()),
            Direction::Backwards => occs.first().and_then(|l| l.occurrence_start()),
        }
        .map(|d| d.to_utc().format("%Y%m%dT%H%M%SZ").to_string())
    } else {
        None
    };

    let html = OccListTemplate {
        locale,
        occs,
        personal_alarms: matches!(alarm_type, CalendarAlarmType::Personal { .. }),
    }
    .render()
    .context("details template")?;

    Ok(Json(Response { html, date }))
}
