use askama::Template;
use chrono::{NaiveDate, NaiveTime};
use ical::objects::CalDate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::html::filters;
use crate::locale::Locale;

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

    pub fn from(&self, locale: &Arc<dyn Locale + Send + Sync>) -> Option<CalDate> {
        self.from.to_caldate(locale, false)
    }

    pub fn to(&self, locale: &Arc<dyn Locale + Send + Sync>) -> Option<CalDate> {
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
        from: Option<CalDate>,
        to: Option<CalDate>,
    ) -> Self {
        Self {
            name,
            id: name.replace("[", "_").replace("]", "_"),
            from_date: from
                .as_ref()
                .map(|d| d.as_start_with_tz(locale.timezone()).date_naive()),
            to_date: to
                .as_ref()
                .map(|d| d.as_end_with_tz(locale.timezone()).date_naive()),
            from_time: match from {
                Some(CalDate::DateTime(ref dt)) => Some(dt.as_naive_time()),
                _ => None,
            },
            to_time: match to {
                Some(CalDate::DateTime(ref dt)) => Some(dt.as_naive_time()),
                _ => None,
            },
            all_day: matches!(from, Some(CalDate::Date(_))),
            locale,
        }
    }
}
