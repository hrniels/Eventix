use askama::Template;
use chrono::{NaiveDate, NaiveTime};
use ical::objects::{CalDate, CalDateTime};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::html::filters;
use crate::locale::Locale;

use super::date::{Date, DateTemplate};

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct DateTime {
    date: Date,
    time: Option<NaiveTime>,
}

impl DateTime {
    pub fn new(date: Date, time: Option<NaiveTime>) -> Self {
        Self { date, time }
    }

    pub fn date(&self) -> Option<NaiveDate> {
        self.date.date()
    }

    pub fn time(&self) -> Option<NaiveTime> {
        self.time
    }

    pub fn to_caldate(&self, locale: &Arc<dyn Locale + Send + Sync>, end: bool) -> Option<CalDate> {
        match self.time {
            Some(time) => Some(CalDate::DateTime(CalDateTime::Timezone(
                self.date.date()?.and_time(time),
                locale.timezone().name().to_string(),
            ))),
            None => Some(self.date.to_caldate(end)?),
        }
    }
}

#[derive(Template)]
#[template(path = "comps/datetime.htm")]
pub struct DateTimeTemplate {
    name: String,
    id: String,
    date: DateTemplate,
    time: Option<NaiveTime>,
}

impl DateTimeTemplate {
    #[allow(dead_code)]
    pub fn new<N: ToString>(name: N, date: Option<DateTime>) -> Self {
        let name = name.to_string();
        Self {
            time: date.as_ref().and_then(|d| d.time()),
            date: DateTemplate::new(format!("{}_date", name), date.map(|d| d.date)),
            id: name.replace("[", "_").replace("]", "_"),
            name,
        }
    }
}
