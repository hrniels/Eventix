// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod index;
mod save;

use axum::{
    Router,
    extract::{RawQuery, State},
    routing::{get, post},
};
use chrono::{Duration, NaiveDateTime, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use eventix_ical::objects::CalCompType;
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::comps::{
    alarm::AlarmRequest, attendees::Attendees, date::Date, datetime::DateTime,
    datetimerange::DateTimeRange, recur::RecurRequest, time::Time, todostatus::TodoStatus,
};
use crate::objects::CompAction;
use crate::pages::{Page, shell};

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    ctype: CalCompType,
    date: Option<Date>,
    hour: Option<u32>,
    allday: Option<bool>,
    prev: Option<String>,
}

#[derive(Default, Debug, Deserialize)]
pub struct CompNew {
    calendar: String,
    summary: String,
    location: String,
    description: String,
    rrule: RecurRequest,
    alarm: AlarmRequest,
    attendees: Option<Attendees>,
    start_end: DateTimeRange,
    status: Option<TodoStatus>,
}

impl CompNew {
    fn new(req: &Request, timezone: &Tz, calendar: Option<String>) -> Self {
        let now = Utc::now().with_timezone(timezone);
        let date = if let Some(date) = &req.date {
            let time = if let Some(hour) = req.hour {
                if hour < 24 {
                    NaiveTime::from_hms_opt(hour, 0, 0).unwrap()
                } else {
                    now.time()
                }
            } else {
                now.time()
            };
            NaiveDateTime::new(date.date().unwrap(), time)
                .and_local_timezone(*timezone)
                .earliest()
                .unwrap()
        } else {
            now
        };

        let start_end = match (req.ctype, req.allday.unwrap_or(false)) {
            // event on given date at the next hour
            (CalCompType::Event, false) => {
                let next_hour = if req.hour.is_some() {
                    date
                } else {
                    let last_hour =
                        NaiveTime::from_hms_opt(date.naive_local().time().hour(), 0, 0).unwrap();
                    date.with_time(last_hour).unwrap() + Duration::hours(1)
                };
                let next_next_hour = next_hour + Duration::hours(1);

                let start = DateTime::new(
                    Date::new(Some(next_hour.date_naive())),
                    Some(Time::new(next_hour.naive_local().time())),
                );

                let end = DateTime::new(
                    Date::new(Some(next_next_hour.date_naive())),
                    Some(Time::new(next_next_hour.naive_local().time())),
                );

                DateTimeRange::new(start, end, timezone.name().to_string())
            }

            // all-day event on given date
            (CalCompType::Event, true) => {
                let start = DateTime::new(Date::new(Some(date.date_naive())), None);
                DateTimeRange::new(start.clone(), start, timezone.name().to_string())
            }

            (CalCompType::Todo, _) => DateTimeRange::default(),
        };

        Self {
            start_end,
            calendar: calendar.unwrap_or_default(),
            status: match req.ctype {
                CalCompType::Todo => Some(TodoStatus::default()),
                CalCompType::Event => None,
            },
            ..Default::default()
        }
    }
}

impl CompAction for CompNew {
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
        Some(&self.rrule)
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

pub async fn new_page(state: &EventixState) -> Page {
    Page::new(state).await
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route(
            "/",
            get(
                |State(state): State<EventixState>, RawQuery(raw): RawQuery| async move {
                    shell::handler(state, raw, "items/add").await
                },
            ),
        )
        .route("/", post(self::save::handler))
        .route("/content", get(self::index::content))
        .with_state(state)
}
