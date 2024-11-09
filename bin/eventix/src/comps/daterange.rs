use askama::Template;
use chrono::{NaiveDate, NaiveTime};
use ical::objects::{CalDate, CalDateTime};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;

use crate::html::filters;
use crate::locale::Locale;

pub fn serialize_date<S>(date: &Option<NaiveDate>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match date {
        Some(date) => serializer.serialize_some(&format!("{}", date.format("%Y-%m-%d"))),
        None => serializer.serialize_none(),
    }
}

pub fn deserialize_date<'de, D>(deserializer: D) -> Result<Option<NaiveDate>, D::Error>
where
    D: Deserializer<'de>,
{
    let buf = String::deserialize(deserializer)?;
    if buf.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            NaiveDate::parse_from_str(&buf, "%Y-%m-%d").map_err(serde::de::Error::custom)?,
        ))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DateRange {
    #[serde(
        serialize_with = "serialize_date",
        deserialize_with = "deserialize_date"
    )]
    from_date: Option<NaiveDate>,
    from_time: Option<NaiveTime>,
    #[serde(
        serialize_with = "serialize_date",
        deserialize_with = "deserialize_date"
    )]
    to_date: Option<NaiveDate>,
    to_time: Option<NaiveTime>,
}

impl DateRange {
    pub fn from(&self, locale: &Arc<dyn Locale + Send + Sync>) -> Option<CalDate> {
        Self::build_caldate(&self.from_date, &self.from_time, locale, false)
    }

    pub fn to(&self, locale: &Arc<dyn Locale + Send + Sync>) -> Option<CalDate> {
        if self.from_time.is_some() && self.to_time.is_none() {
            return None;
        }

        Self::build_caldate(&self.to_date, &self.to_time, locale, true)
    }

    fn build_caldate(
        date: &Option<NaiveDate>,
        time: &Option<NaiveTime>,
        locale: &Arc<dyn Locale + Send + Sync>,
        end: bool,
    ) -> Option<CalDate> {
        let mut date = *date.as_ref()?;
        match time {
            Some(time) => Some(CalDate::DateTime(CalDateTime::Timezone(
                date.and_time(*time),
                locale.timezone().name().to_string(),
            ))),
            None => {
                if end {
                    date = date.succ_opt()?;
                }
                Some(CalDate::Date(date))
            }
        }
    }
}

#[derive(Template)]
#[template(path = "comps/daterange.htm")]
pub struct DateRangeTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    name: &'a str,
    from_date: Option<NaiveDate>,
    to_date: Option<NaiveDate>,
    from_time: Option<NaiveTime>,
    to_time: Option<NaiveTime>,
    all_day: bool,
}

impl<'a> DateRangeTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        name: &'a str,
        from: Option<CalDate>,
        to: Option<CalDate>,
    ) -> DateRangeTemplate<'a> {
        Self {
            name,
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
