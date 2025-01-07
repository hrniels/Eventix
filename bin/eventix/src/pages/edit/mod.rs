mod index;
mod update;

use axum::{
    routing::{get, post},
    Router,
};
use chrono_tz::Tz;
use ical::objects::EventLike;
use ical::{col::Occurrence, objects::CalCompType};
use serde::{Deserialize, Serialize};

use crate::{
    comp::CompAction,
    comps::{
        alarm::AlarmRequest, attendees::Attendees, datetimerange::DateTimeRange,
        recur::RecurRequest, todostatus::TodoStatus,
    },
    state::State,
};

use super::Page;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompEdit {
    #[serde(flatten)]
    req: Request,
    summary: String,
    location: String,
    description: String,
    rrule: Option<RecurRequest>,
    reminder: AlarmRequest,
    start_end: DateTimeRange,
    attendees: Option<Attendees>,
    status: Option<TodoStatus>,
}

impl CompEdit {
    pub fn new_from_occurrence(req: Request, occ: &Occurrence, tz: &Tz) -> Self {
        Self {
            summary: occ.summary().cloned().unwrap_or(String::from("")),
            location: occ.location().cloned().unwrap_or(String::from("")),
            description: occ.description().cloned().unwrap_or(String::from("")),
            rrule: if req.rid.is_none() {
                Some(RecurRequest::from_rrule(occ.rrule()))
            } else {
                None
            },
            reminder: if !occ.alarms().is_empty() {
                AlarmRequest::from_alarm(occ.alarms(), tz)
            } else {
                AlarmRequest::default()
            },
            start_end: DateTimeRange::new_from_caldate(
                if occ.is_recurrent() {
                    Some(occ.occurrence_startdate())
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
            req,
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
    fn reminder(&self) -> &AlarmRequest {
        &self.reminder
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

pub async fn new_page(state: &State) -> Page {
    Page::new(state).await
}

pub fn path() -> &'static str {
    "/edit"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/", get(self::index::handler))
        .route("/update", post(self::update::handler))
        .with_state(state)
}
