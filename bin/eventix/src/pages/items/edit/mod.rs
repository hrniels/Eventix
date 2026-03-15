// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod index;
mod update;

use axum::{
    Router,
    extract::{RawQuery, State},
    routing::{get, post},
};
use chrono_tz::Tz;
use eventix_ical::col::Occurrence;
use eventix_ical::objects::{CalAlarm, CalCompType, EventLike};
use eventix_state::EventixState;
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, time::SystemTime};

use crate::comps::{
    alarm::AlarmRequest, attendees::Attendees, datetimerange::DateTimeRange, recur::RecurRequest,
    todostatus::TodoStatus,
};
use crate::objects::CompAction;
use crate::pages::{Page, shell};
use crate::util;

pub fn deserialize_u128<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    let buf = String::deserialize(deserializer)?;
    let num = buf.parse::<u128>().map_err(serde::de::Error::custom)?;
    Ok(num)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EditMode {
    Occurrence,
    Following,
    Series,
}

impl fmt::Display for EditMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    mode: EditMode,
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
            rrule: if req.mode != EditMode::Occurrence {
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

pub fn build_title(occ: &Occurrence, rid: &Option<String>, mode: EditMode) -> String {
    let mut title = String::from("Edit ");
    if mode == EditMode::Following {
        title.push_str("Following ");
    }
    match occ.ctype() {
        CalCompType::Event => title.push_str("Event"),
        CalCompType::Todo => title.push_str("Task"),
    }
    if mode == EditMode::Following {
        title.push_str(" Occurrences");
    } else if rid.is_some() {
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
        .route(
            "/",
            get(
                |State(state): State<EventixState>, RawQuery(raw): RawQuery| async move {
                    shell::handler(state, raw, "items/edit").await
                },
            ),
        )
        .route("/", post(self::update::handler))
        .route("/content", get(self::index::content))
        .with_state(state)
}
