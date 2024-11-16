use anyhow::anyhow;
use askama::Template;
use chrono::Weekday;
use ical::objects::{CalRRule, CalRRuleFreq, CalWDayDesc};
use serde::{Deserialize, Deserializer};
use std::fmt;
use std::sync::Arc;
use strum::EnumIter;

use crate::{comps::date::DateTemplate, html::filters, locale::Locale};

use super::date::Date;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Frequency {
    pub fn to_cal_freq(&self) -> CalRRuleFreq {
        match self {
            Frequency::Daily => CalRRuleFreq::Daily,
            Frequency::Weekly => CalRRuleFreq::Weekly,
            Frequency::Monthly => CalRRuleFreq::Monthly,
            Frequency::Yearly => CalRRuleFreq::Yearly,
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Frequency>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf = String::deserialize(deserializer)?;
        match buf.as_str() {
            "DAILY" => Ok(Some(Frequency::Daily)),
            "WEEKLY" => Ok(Some(Frequency::Weekly)),
            "MONTHLY" => Ok(Some(Frequency::Monthly)),
            "YEARLY" => Ok(Some(Frequency::Yearly)),
            _ => Ok(None),
        }
    }
}

#[derive(Debug, EnumIter, Eq, PartialEq)]
enum IterWeekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl From<Weekday> for IterWeekday {
    fn from(wday: Weekday) -> Self {
        match wday {
            Weekday::Mon => Self::Monday,
            Weekday::Tue => Self::Tuesday,
            Weekday::Wed => Self::Wednesday,
            Weekday::Thu => Self::Thursday,
            Weekday::Fri => Self::Friday,
            Weekday::Sat => Self::Saturday,
            Weekday::Sun => Self::Sunday,
        }
    }
}

impl fmt::Display for IterWeekday {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

fn parse_by_day(wdays: &str) -> Vec<CalWDayDesc> {
    let mut days = vec![];
    println!("wdays = {}", wdays);
    for day in wdays.split(',') {
        println!("!! {} !!", day);
        if let Ok(wday) = CalWDayDesc::parse_weekday(&day) {
            days.push(CalWDayDesc::new(wday, None));
        }
    }
    days
}

#[derive(Debug, Deserialize)]
pub enum RecurEnd {
    NoEnd,
    Count,
    Until,
}

#[derive(Debug, Deserialize)]
pub struct RecurRequest {
    #[serde(deserialize_with = "Frequency::deserialize")]
    freq: Option<Frequency>,
    interval: Option<u8>,
    end: RecurEnd,
    count: u8,
    until: Option<Date>,
    weekdays: String,
}

impl RecurRequest {
    pub fn to_rrule(&self) -> anyhow::Result<Option<CalRRule>> {
        if let Some(freq) = self.freq {
            let mut rrule = CalRRule::default();
            rrule.set_frequency(freq.to_cal_freq());
            rrule.set_interval(self.interval.unwrap());

            match freq {
                Frequency::Weekly => {
                    let byday = parse_by_day(&self.weekdays);
                    rrule.set_by_day(byday);
                }
                _ => {}
            }

            match self.end {
                RecurEnd::Count => rrule.set_count(self.count),
                RecurEnd::Until => {
                    if let Some(ref until) = self.until {
                        rrule.set_until(
                            until
                                .to_caldate(false)
                                .ok_or_else(|| anyhow!("Please specify a valid end date"))?,
                        );
                    } else {
                        return Err(anyhow!("Please specify the end date"));
                    }
                }
                RecurEnd::NoEnd => {}
            }

            Ok(Some(rrule))
        } else {
            Ok(None)
        }
    }
}

#[derive(Template)]
#[template(path = "comps/recur.htm")]
pub struct RecurTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    name: &'a str,
    id: String,
    freq: String,
    count: String,
    interval: String,
    weekdays: String,
    end: &'a str,
    until: DateTemplate,
}

impl<'a> RecurTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        name: &'a str,
        rrule: Option<&CalRRule>,
    ) -> Self {
        let freq = match rrule {
            Some(r) => format!("{}", r.frequency()),
            None => "NONE".to_string(),
        };
        let count = match rrule {
            Some(r) if r.count().is_some() => format!("{}", r.count().unwrap()),
            _ => "1".to_string(),
        };
        let end = match rrule {
            Some(r) if r.count().is_some() => "count",
            Some(r) if r.until().is_some() => "until",
            _ => "none",
        };

        let weekdays = match rrule {
            Some(r) if r.by_day().is_some() => {
                let mut wdays = String::new();
                for wd in r.by_day().as_ref().unwrap().iter() {
                    wdays.push_str(&format!("{},", CalWDayDesc::to_weekday_str(wd.day())));
                }
                wdays
            }
            _ => "".to_string(),
        };

        Self {
            name,
            id: name.replace("[", "_").replace("]", "_"),
            freq,
            count,
            interval: rrule
                .and_then(|r| r.interval().map(|i| format!("{}", i)))
                .unwrap_or(String::from("")),
            weekdays,
            end,
            until: DateTemplate::new(
                format!("{}[until]", name),
                rrule.and_then(|r| r.until()).cloned(),
            ),
            locale,
        }
    }
}
