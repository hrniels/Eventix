use askama::Template;
use chrono::NaiveDate;
use chrono_tz::Tz;
use eventix_ical::objects::{CalCompType, CalDate, CalDateType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::comps::date::Date;
use crate::comps::datetime::DateTime;
use crate::comps::time::{Time, TimeTemplate};
use crate::html::filters;
use crate::locale::Locale;

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct DateTimeRange {
    from: DateTime,
    to: DateTime,
    from_enabled: Option<bool>,
    to_enabled: Option<bool>,
}

impl DateTimeRange {
    pub fn new(from: DateTime, to: DateTime) -> Self {
        Self {
            from_enabled: from.date().map(|_| true),
            to_enabled: to.date().map(|_| true),
            from,
            to,
        }
    }

    pub fn new_from_caldate(from: Option<CalDate>, to: Option<CalDate>, tz: &Tz) -> Self {
        Self {
            from: DateTime::new(
                Date::new(from.as_ref().map(|f| f.as_start_with_tz(tz).date_naive())),
                from.as_ref().and_then(|f| match f {
                    CalDate::DateTime(dt) => Some(Time::new(dt.with_tz(tz).time())),
                    CalDate::Date(..) => None,
                }),
            ),
            to: DateTime::new(
                Date::new(to.as_ref().map(|t| t.as_end_with_tz(tz).date_naive())),
                to.as_ref().and_then(|t| match t {
                    CalDate::DateTime(dt) => Some(Time::new(dt.with_tz(tz).time())),
                    CalDate::Date(..) => None,
                }),
            ),
            from_enabled: from.map(|_| true),
            to_enabled: to.map(|_| true),
        }
    }

    pub fn is_all_day(&self) -> bool {
        self.from.time().is_none() && self.to.time().is_none()
    }

    pub fn as_caldates(
        &self,
        locale: &Arc<dyn Locale + Send + Sync>,
        ty: CalDateType,
    ) -> (Option<CalDate>, Option<CalDate>) {
        (
            if self.from_enabled.is_none() {
                None
            } else {
                self.from.to_caldate(locale, ty, false)
            },
            if self.to_enabled.is_none() {
                None
            } else {
                self.to.to_caldate(locale, ty, true)
            },
        )
    }
}

#[derive(Template)]
#[template(path = "comps/datetimerange.htm")]
pub struct DateTimeRangeTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    ctype: CalCompType,
    name: &'a str,
    id: String,
    from_date: Option<NaiveDate>,
    to_date: Option<NaiveDate>,
    from_time: TimeTemplate,
    to_time: TimeTemplate,
    from_enabled: bool,
    to_enabled: bool,
    all_day: bool,
}

impl<'a> DateTimeRangeTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        ctype: CalCompType,
        name: &'a str,
        value: Option<DateTimeRange>,
    ) -> Self {
        Self {
            locale,
            ctype,
            name,
            id: name.replace("[", "_").replace("]", "_"),
            from_date: value.as_ref().and_then(|v| v.from.date()),
            to_date: value.as_ref().and_then(|v| v.to.date()),
            from_time: TimeTemplate::new(
                format!("{name}[from][time]"),
                value.as_ref().and_then(|v| v.from.time()),
            ),
            to_time: TimeTemplate::new(
                format!("{name}[to][time]"),
                value.as_ref().and_then(|v| v.to.time()),
            ),
            all_day: value.as_ref().map(|v| v.is_all_day()).unwrap_or(false),
            from_enabled: value
                .as_ref()
                .map(|v| v.from_enabled.is_some())
                .unwrap_or(false),
            to_enabled: value
                .as_ref()
                .map(|v| v.to_enabled.is_some())
                .unwrap_or(false),
        }
    }
}
