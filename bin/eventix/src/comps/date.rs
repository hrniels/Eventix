use askama::Template;
use chrono::NaiveDate;
use ical::objects::{CalDate, CalDateType};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::html::filters;

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

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Date {
    #[serde(
        serialize_with = "serialize_date",
        deserialize_with = "deserialize_date"
    )]
    date: Option<NaiveDate>,
}

impl Date {
    pub fn new(date: Option<NaiveDate>) -> Self {
        Self { date }
    }

    pub fn date(&self) -> Option<NaiveDate> {
        self.date
    }

    pub fn to_caldate(&self, ty: CalDateType, end: bool) -> Option<CalDate> {
        let date = if ty == CalDateType::Exclusive && end {
            self.date.and_then(|d| d.succ_opt())
        } else {
            self.date
        };
        date.map(|d| CalDate::Date(d, ty))
    }
}

#[derive(Template)]
#[template(path = "comps/date.htm")]
pub struct DateTemplate {
    name: String,
    id: String,
    date: Option<NaiveDate>,
}

impl DateTemplate {
    pub fn new<N: ToString>(name: N, date: Option<Date>) -> Self {
        let name = name.to_string();
        Self {
            id: name.replace("[", "_").replace("]", "_"),
            name,
            date: date.and_then(|d| d.date),
        }
    }
}
