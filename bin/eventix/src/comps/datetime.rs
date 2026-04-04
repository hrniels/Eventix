// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use chrono::NaiveDate;
use chrono_tz::Tz;
use eventix_ical::objects::{CalDate, CalDateTime, CalDateType};
use serde::{Deserialize, Deserializer, Serialize};

use crate::comps::date::{Date, DateTemplate};
use crate::comps::time::{Time, TimeTemplate};

/// Deserializes an optional time from a form field, treating an empty string as `None`.
///
/// HTML time inputs submit an empty string when left blank. `NaiveTime`'s built-in
/// deserializer rejects empty strings, so this helper maps them to `None` instead.
fn deserialize_time<'de, D>(deserializer: D) -> Result<Option<Time>, D::Error>
where
    D: Deserializer<'de>,
{
    let buf = String::deserialize(deserializer)?;
    if buf.is_empty() {
        Ok(None)
    } else {
        buf.parse()
            .map(|t| Some(Time::new(t)))
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct DateTime {
    date: Date,
    #[serde(default, deserialize_with = "deserialize_time")]
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

    pub fn to_caldate(&self, timezone: &str, ty: CalDateType, end: bool) -> Option<CalDate> {
        match &self.time {
            Some(time) => Some(CalDate::DateTime(CalDateTime::Timezone(
                self.date.date()?.and_time(time.value()),
                timezone.to_string(),
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
