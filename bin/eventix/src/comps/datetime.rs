use askama::Template;
use chrono::NaiveDate;
use chrono_tz::Tz;
use eventix_ical::objects::{CalDate, CalDateTime, CalDateType};
use eventix_locale::Locale;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::comps::date::{Date, DateTemplate};
use crate::comps::time::{Time, TimeTemplate};

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct DateTime {
    date: Date,
    time: Option<Time>,
}

impl DateTime {
    pub fn from_caldate(date: &CalDate, timezone: &Tz) -> Self {
        let dt = date.as_start_with_tz(timezone);
        Self::new(Date::new(Some(dt.date_naive())), Some(Time::new(dt.time())))
    }

    pub fn new(date: Date, time: Option<Time>) -> Self {
        Self { date, time }
    }

    pub fn date(&self) -> Option<NaiveDate> {
        self.date.date()
    }

    pub fn time(&self) -> Option<Time> {
        self.time.clone()
    }

    pub fn to_caldate(
        &self,
        locale: &Arc<dyn Locale + Send + Sync>,
        ty: CalDateType,
        end: bool,
    ) -> Option<CalDate> {
        match &self.time {
            Some(time) => Some(CalDate::DateTime(CalDateTime::Timezone(
                self.date.date()?.and_time(time.value()),
                locale.timezone().name().to_string(),
            ))),
            None => Some(self.date.to_caldate(ty, end)?),
        }
    }
}

#[derive(Template)]
#[template(path = "comps/datetime.htm")]
pub struct DateTimeTemplate {
    date: DateTemplate,
    time: TimeTemplate,
}

impl DateTimeTemplate {
    #[allow(dead_code)]
    pub fn new<N: ToString>(name: N, date: Option<DateTime>) -> Self {
        let name = name.to_string();
        Self {
            time: TimeTemplate::new(
                format!("{name}[time]"),
                date.as_ref().and_then(|d| d.time()),
            ),
            date: DateTemplate::new(format!("{name}[date]"), date.map(|d| d.date)),
        }
    }
}
