mod index;
mod update;

use axum::{
    Router,
    routing::{get, post},
};
use chrono_tz::Tz;
use ical::objects::{CalAlarm, EventLike};
use ical::{col::Occurrence, objects::CalCompType};
use serde::{Deserialize, Deserializer, Serialize};
use std::time::SystemTime;

use crate::{
    comps::{
        alarm::AlarmRequest, attendees::Attendees, datetimerange::DateTimeRange,
        recur::RecurRequest, todostatus::TodoStatus,
    },
    objects::CompAction,
    state::EventixState,
    util,
};

use super::Page;

pub fn deserialize_u128<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    let buf = String::deserialize(deserializer)?;
    let num = buf.parse::<u128>().map_err(serde::de::Error::custom)?;
    Ok(num)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
    prev: String,
}

#[derive(Debug, Deserialize)]
pub struct CompEdit {
    #[serde(deserialize_with = "deserialize_u128")]
    edit_start: u128,
    calendar: Option<String>,
    summary: String,
    location: String,
    description: String,
    rrule: Option<RecurRequest>,
    alarm: AlarmRequest,
    start_end: DateTimeRange,
    attendees: Option<Attendees>,
    status: Option<TodoStatus>,
}

impl CompEdit {
    pub fn new_from_occurrence(
        req: &Request,
        occ: &Occurrence,
        pers_alarms: Option<&[CalAlarm]>,
        calendar: Option<String>,
        tz: &Tz,
    ) -> Self {
        Self {
            calendar,
            edit_start: util::system_time_stamp(SystemTime::now()),
            summary: occ.summary().cloned().unwrap_or(String::from("")),
            location: occ.location().cloned().unwrap_or(String::from("")),
            description: occ.description().cloned().unwrap_or(String::from("")),
            rrule: if req.rid.is_none() {
                Some(RecurRequest::from_rrule(occ.rrule()))
            } else {
                None
            },
            alarm: AlarmRequest::from_alarms(occ.alarms().unwrap_or_default(), pers_alarms, tz),
            start_end: DateTimeRange::new_from_caldate(
                if occ.is_recurrent() {
                    occ.occurrence_startdate()
                } else {
                    occ.start().cloned()
                },
                if occ.is_recurrent() {
                    occ.occurrence_enddate()
                } else {
                    occ.end_or_due().cloned()
                },
                tz,
            ),
            status: if !occ.is_recurrent() || req.rid.is_some() {
                TodoStatus::new_from_occurrence(occ)
            } else {
                None
            },
            attendees: Some(Attendees::new_from_cal_attendees(occ.attendees())),
        }
    }
}

impl CompAction for CompEdit {
    fn summary(&self) -> &String {
        &self.summary
    }
    fn location(&self) -> &String {
        &self.location
    }
    fn description(&self) -> &String {
        &self.description
    }
    fn rrule(&self) -> Option<&RecurRequest> {
        self.rrule.as_ref()
    }
    fn start_end(&self) -> &DateTimeRange {
        &self.start_end
    }
    fn alarm(&self) -> &AlarmRequest {
        &self.alarm
    }
    fn attendees(&self) -> Option<&Attendees> {
        self.attendees.as_ref()
    }
    fn status(&self) -> Option<&TodoStatus> {
        self.status.as_ref()
    }
}

pub fn build_title(occ: &Occurrence, rid: &Option<String>) -> String {
    let mut title = String::from("Edit ");
    match occ.ctype() {
        CalCompType::Event => title.push_str("Event"),
        CalCompType::Todo => title.push_str("Task"),
    }
    if rid.is_some() {
        title.push_str(" Occurrence");
    } else if occ.rrule().is_some() {
        title.push_str(" Series");
    }
    title
}

pub async fn new_page(state: &EventixState) -> Page {
    Page::new(state).await
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/edit", get(self::index::handler))
        .route("/edit", post(self::update::handler))
        .with_state(state)
}
