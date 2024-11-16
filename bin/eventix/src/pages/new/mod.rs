mod index;
mod save;

use axum::{
    routing::{get, post},
    Router,
};
use chrono::{Duration, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use ical::col::Id;
use serde::Deserialize;

use crate::comps::{
    date::Date, datetime::DateTime, datetimerange::DateTimeRange, recur::RecurRequest,
};

use super::{Breadcrumb, Page};

#[derive(Default, Debug, Deserialize)]
pub struct Save {
    calendar: Id,
    summary: String,
    location: String,
    description: String,
    rrule: RecurRequest,
    start_end: DateTimeRange,
}

impl Save {
    fn new(timezone: &Tz) -> Self {
        let now = Utc::now().with_timezone(timezone);
        let last_hour = NaiveTime::from_hms_opt(now.naive_local().time().hour(), 0, 0).unwrap();
        let next_hour = now.with_time(last_hour).unwrap() + Duration::hours(1);
        let next_next_hour = next_hour + Duration::hours(1);

        let start = DateTime::new(
            Date::new(Some(next_hour.date_naive())),
            Some(next_hour.naive_local().time()),
        );
        let end = DateTime::new(
            Date::new(Some(next_next_hour.date_naive())),
            Some(next_next_hour.naive_local().time()),
        );

        Self {
            start_end: DateTimeRange::new(start, end),
            ..Default::default()
        }
    }
}

pub fn new_page() -> Page {
    let mut page = Page::new(path().to_string());
    page.add_breadcrumb(Breadcrumb::new(format!("{}", path()), "New event"));
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
