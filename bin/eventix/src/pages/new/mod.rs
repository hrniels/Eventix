mod index;
mod save;

use axum::{
    routing::{get, post},
    Router,
};
use chrono::{Duration, NaiveDateTime, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use ical::objects::CalCompType;
use serde::{Deserialize, Serialize};

use crate::{
    comp::CompAction,
    comps::{
        alarm::AlarmRequest, attendees::Attendees, date::Date, datetime::DateTime,
        datetimerange::DateTimeRange, recur::RecurRequest, time::Time, todostatus::TodoStatus,
    },
    state::EventixState,
};

use super::{Breadcrumb, Page};

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    ctype: CalCompType,
    date: Option<Date>,
    hour: Option<u32>,
}

#[derive(Default, Debug, Deserialize)]
pub struct CompNew {
    #[serde(flatten)]
    req: Request,
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

        let next_hour = if req.hour.is_some() {
            date
        } else {
            let last_hour =
                NaiveTime::from_hms_opt(date.naive_local().time().hour(), 0, 0).unwrap();
            date.with_time(last_hour).unwrap() + Duration::hours(1)
        };
        let next_next_hour = next_hour + Duration::hours(1);

        let start_end = match req.ctype {
            CalCompType::Event => {
                let start = DateTime::new(
                    Date::new(Some(next_hour.date_naive())),
                    Some(Time::new(next_hour.naive_local().time())),
                );
                let end = DateTime::new(
                    Date::new(Some(next_next_hour.date_naive())),
                    Some(Time::new(next_next_hour.naive_local().time())),
                );
                DateTimeRange::new(start, end)
            }
            CalCompType::Todo => DateTimeRange::default(),
        };

        Self {
            req: Request {
                ctype: req.ctype,
                date: None,
                hour: None,
            },
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

pub async fn new_page(state: &EventixState, req: &Request) -> Page {
    let mut page = Page::new(state).await;
    match req.ctype {
        CalCompType::Todo => page.add_breadcrumb(Breadcrumb::new(
            format!("/new?ctype={:?}", req.ctype),
            "New task",
        )),
        CalCompType::Event => page.add_breadcrumb(Breadcrumb::new(
            format!("/new?ctype={:?}", req.ctype),
            "New event",
        )),
    }
    page
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/new", get(self::index::handler))
        .route("/new/save", post(self::save::handler))
        .with_state(state)
}
