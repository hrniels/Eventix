mod index;
mod save;

use axum::{
    routing::{get, post},
    Router,
};
use chrono::{Duration, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use ical::objects::CalCompType;
use serde::{Deserialize, Serialize};

use crate::{
    comp::CompAction,
    comps::{
        alarm::AlarmRequest, attendees::Attendees, date::Date, datetime::DateTime,
        datetimerange::DateTimeRange, recur::RecurRequest,
    },
    state::State,
};

use super::{Breadcrumb, Page};

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    ctype: CalCompType,
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
    reminder: AlarmRequest,
    attendees: Option<Attendees>,
    start_end: DateTimeRange,
}

impl CompNew {
    fn new(ctype: CalCompType, timezone: &Tz) -> Self {
        let now = Utc::now().with_timezone(timezone);
        let last_hour = NaiveTime::from_hms_opt(now.naive_local().time().hour(), 0, 0).unwrap();
        let next_hour = now.with_time(last_hour).unwrap() + Duration::hours(1);
        let next_next_hour = next_hour + Duration::hours(1);

        let start_end = match ctype {
            CalCompType::Event => {
                let start = DateTime::new(
                    Date::new(Some(next_hour.date_naive())),
                    Some(next_hour.naive_local().time()),
                );
                let end = DateTime::new(
                    Date::new(Some(next_next_hour.date_naive())),
                    Some(next_next_hour.naive_local().time()),
                );
                DateTimeRange::new(start, end)
            }
            CalCompType::Todo => DateTimeRange::default(),
        };

        Self {
            req: Request { ctype },
            start_end,
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
    fn reminder(&self) -> &AlarmRequest {
        &self.reminder
    }
    fn attendees(&self) -> Option<&Attendees> {
        self.attendees.as_ref()
    }
}

pub async fn new_page(state: &State, req: &Request) -> Page {
    let mut page = Page::new(state).await;
    match req.ctype {
        CalCompType::Todo => page.add_breadcrumb(Breadcrumb::new(
            format!("{}?ctype={:?}", path(), req.ctype),
            "New task",
        )),
        CalCompType::Event => page.add_breadcrumb(Breadcrumb::new(
            format!("{}?ctype={:?}", path(), req.ctype),
            "New event",
        )),
    }
    page
}

pub fn path() -> &'static str {
    "/new"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/", get(self::index::handler))
        .route("/save", post(self::save::handler))
        .with_state(state)
}
