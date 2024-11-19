use askama::Template;
use chrono::{NaiveDate, NaiveTime};
use chrono_tz::Tz;
use ical::objects::CalDate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::html::filters;
use crate::locale::Locale;

use super::date::Date;
use super::datetime::DateTime;

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct DateTimeRange {
    from: DateTime,
    to: DateTime,
}

impl DateTimeRange {
    pub fn new(from: DateTime, to: DateTime) -> Self {
        Self { from, to }
    }

    pub fn new_from_caldate(from: Option<CalDate>, to: Option<CalDate>, tz: &Tz) -> Self {
        Self {
            from: DateTime::new(
                Date::new(from.as_ref().map(|f| f.as_start_with_tz(tz).date_naive())),
                from.as_ref().and_then(|f| match f {
                    CalDate::DateTime(dt) => Some(dt.as_naive_time()),
                    CalDate::Date(_) => None,
                }),
            ),
            to: DateTime::new(
                Date::new(to.as_ref().map(|t| t.as_end_with_tz(tz).date_naive())),
                to.as_ref().and_then(|t| match t {
                    CalDate::DateTime(dt) => Some(dt.as_naive_time()),
                    CalDate::Date(_) => None,
                }),
            ),
        }
    }

    pub fn is_all_day(&self) -> bool {
        self.from.time().is_none() && self.to.time().is_none()
    }

    pub fn from(&self) -> &DateTime {
        &self.from
    }

    pub fn to(&self) -> &DateTime {
        &self.to
    }

    pub fn from_as_caldate(&self, locale: &Arc<dyn Locale + Send + Sync>) -> Option<CalDate> {
        self.from.to_caldate(locale, false)
    }

    pub fn to_as_caldate(&self, locale: &Arc<dyn Locale + Send + Sync>) -> Option<CalDate> {
        if self.from.time().is_some() != self.to.time().is_some() {
            return None;
        }
        self.to.to_caldate(locale, true)
    }
}

#[derive(Template)]
#[template(path = "comps/datetimerange.htm")]
pub struct DateTimeRangeTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    name: &'a str,
    id: String,
    from_date: Option<NaiveDate>,
    to_date: Option<NaiveDate>,
    from_time: Option<NaiveTime>,
    to_time: Option<NaiveTime>,
    all_day: bool,
}

impl<'a> DateTimeRangeTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        name: &'a str,
        value: Option<DateTimeRange>,
    ) -> Self {
        Self {
            name,
            id: name.replace("[", "_").replace("]", "_"),
            from_date: value.as_ref().and_then(|v| v.from().date()),
            to_date: value.as_ref().and_then(|v| v.to().date()),
            from_time: value.as_ref().and_then(|v| v.from().time()),
            to_time: value.as_ref().and_then(|v| v.to().time()),
            all_day: value.as_ref().map(|v| v.is_all_day()).unwrap_or(false),
            locale,
        }
    }
}
