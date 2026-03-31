// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::anyhow;
use askama::Template;
use chrono::Weekday;
use chrono_tz::Tz;
use eventix_ical::objects::{CalDateType, CalRRule, CalRRuleFreq, CalRRuleSide, CalWDayDesc};
use eventix_ical::parser::ParseError;
use eventix_locale::Locale;
use serde::{Deserialize, Deserializer};
use std::fmt::{self, Display};
use std::sync::Arc;
use strum::EnumIter;

use crate::comps::{combobox::ComboboxTemplate, combobox::Named, date::Date, date::DateTemplate};
use crate::html::filters;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Frequency {
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Display for Frequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Frequency::Hourly => write!(f, "HOURLY"),
            Frequency::Daily => write!(f, "DAILY"),
            Frequency::Weekly => write!(f, "WEEKLY"),
            Frequency::Monthly => write!(f, "MONTHLY"),
            Frequency::Yearly => write!(f, "YEARLY"),
        }
    }
}

impl TryFrom<CalRRuleFreq> for Frequency {
    type Error = ();

    fn try_from(value: CalRRuleFreq) -> Result<Self, Self::Error> {
        match value {
            CalRRuleFreq::Secondly => Err(()),
            CalRRuleFreq::Minutely => Err(()),
            CalRRuleFreq::Hourly => Ok(Self::Hourly),
            CalRRuleFreq::Daily => Ok(Self::Daily),
            CalRRuleFreq::Weekly => Ok(Self::Weekly),
            CalRRuleFreq::Monthly => Ok(Self::Monthly),
            CalRRuleFreq::Yearly => Ok(Self::Yearly),
        }
    }
}

impl From<Frequency> for CalRRuleFreq {
    fn from(value: Frequency) -> Self {
        match value {
            Frequency::Hourly => Self::Hourly,
            Frequency::Daily => Self::Daily,
            Frequency::Weekly => Self::Weekly,
            Frequency::Monthly => Self::Monthly,
            Frequency::Yearly => Self::Yearly,
        }
    }
}

impl Frequency {
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Frequency>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf = String::deserialize(deserializer)?;
        match buf.as_str() {
            "HOURLY" => Ok(Some(Frequency::Hourly)),
            "DAILY" => Ok(Some(Frequency::Daily)),
            "WEEKLY" => Ok(Some(Frequency::Weekly)),
            "MONTHLY" => Ok(Some(Frequency::Monthly)),
            "YEARLY" => Ok(Some(Frequency::Yearly)),
            _ => Ok(None),
        }
    }
}

#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq, Deserialize)]
enum IterWeekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl From<IterWeekday> for Weekday {
    fn from(wday: IterWeekday) -> Self {
        match wday {
            IterWeekday::Monday => Self::Mon,
            IterWeekday::Tuesday => Self::Tue,
            IterWeekday::Wednesday => Self::Wed,
            IterWeekday::Thursday => Self::Thu,
            IterWeekday::Friday => Self::Fri,
            IterWeekday::Saturday => Self::Sat,
            IterWeekday::Sunday => Self::Sun,
        }
    }
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

impl Named for IterWeekday {
    fn name(&self, locale: &dyn Locale) -> String {
        locale.translate(&format!("{self:?}")).to_string()
    }
}

impl fmt::Display for IterWeekday {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, EnumIter, Eq, PartialEq, Deserialize)]
enum Nth {
    First,
    Second,
    Third,
    Last,
}

impl Named for Nth {
    fn name(&self, locale: &dyn Locale) -> String {
        locale.translate(&format!("{self:?}")).to_string()
    }
}

impl fmt::Display for Nth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

fn monthly_nth_from_rrule(rrule: Option<&CalRRule>) -> Option<Nth> {
    match rrule {
        Some(r) if r.by_day().is_some() && !r.by_day().as_ref().unwrap().is_empty() => {
            r.by_day().unwrap()[0]
                .nth()
                .and_then(|(num, side)| match side {
                    CalRRuleSide::Start => match num {
                        1 => Some(Nth::First),
                        2 => Some(Nth::Second),
                        3 => Some(Nth::Third),
                        _ => None,
                    },
                    CalRRuleSide::End if num == 1 => Some(Nth::Last),
                    _ => None,
                })
        }
        _ => None,
    }
}

fn parse_by_day(wdays: &str) -> Option<Vec<CalWDayDesc>> {
    let mut days = vec![];
    for day in wdays.split(',') {
        if let Ok(wday) = CalWDayDesc::parse_weekday(day) {
            days.push(CalWDayDesc::new(wday, None));
        }
    }
    if days.is_empty() { None } else { Some(days) }
}

#[derive(Default, Debug, Deserialize, PartialEq, Eq)]
pub enum RecurEnd {
    #[default]
    NoEnd,
    Count,
    Until,
}

impl Display for RecurEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Default, Debug, Deserialize, PartialEq, Eq)]
pub enum MonthlyType {
    #[default]
    None,
    ByDay,
}

impl Display for MonthlyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Deserialize)]
pub struct RecurRequest {
    #[serde(deserialize_with = "Frequency::deserialize")]
    freq: Option<Frequency>,
    interval: u8,
    end: RecurEnd,
    count: u64,
    until: Option<Date>,
    weekly_days: String,
    monthly_type: MonthlyType,
    monthly_nth: Option<Nth>,
    monthly_wday: Option<IterWeekday>,
}

impl Default for RecurRequest {
    fn default() -> Self {
        Self {
            freq: None,
            interval: 1,
            count: 1,
            end: RecurEnd::NoEnd,
            until: None,
            weekly_days: String::default(),
            monthly_type: MonthlyType::None,
            monthly_nth: None,
            monthly_wday: None,
        }
    }
}

impl RecurRequest {
    pub fn from_rrule(rrule: Option<&CalRRule>) -> Self {
        Self {
            freq: rrule.and_then(|r| Frequency::try_from(r.frequency()).ok()),
            interval: rrule.and_then(|r| r.interval()).unwrap_or(1),
            count: rrule.and_then(|r| r.count()).unwrap_or(1),
            end: match rrule {
                Some(r) if r.count().is_some() => RecurEnd::Count,
                Some(r) if r.until().is_some() => RecurEnd::Until,
                _ => RecurEnd::NoEnd,
            },
            until: rrule
                .and_then(|r| r.until())
                .map(|d| Date::new(Some(d.as_naive_date()))),
            weekly_days: match rrule {
                Some(r) if r.by_day().is_some() => {
                    let mut wdays = String::new();
                    for wd in r.by_day().as_ref().unwrap().iter() {
                        wdays.push_str(&format!("{},", CalWDayDesc::to_weekday_str(wd.day())));
                    }
                    wdays
                }
                _ => "".to_string(),
            },
            monthly_type: match rrule {
                Some(r) if r.by_day().is_some() => MonthlyType::ByDay,
                _ => MonthlyType::None,
            },
            monthly_nth: monthly_nth_from_rrule(rrule),
            monthly_wday: rrule.and_then(|r| {
                r.by_day().and_then(|d| {
                    if d.is_empty() {
                        None
                    } else {
                        Some(d[0].day().into())
                    }
                })
            }),
        }
    }

    pub fn to_rrule(&self) -> anyhow::Result<Option<CalRRule>> {
        if let Some(freq) = self.freq {
            let mut rrule = CalRRule::default();
            rrule.set_frequency(freq.into());
            rrule.set_interval(self.interval);

            match freq {
                Frequency::Weekly => {
                    let byday = parse_by_day(&self.weekly_days);
                    rrule.set_by_day(byday);
                }
                Frequency::Monthly => {
                    if self.monthly_type == MonthlyType::ByDay {
                        let nth = match self.monthly_nth.as_ref().unwrap() {
                            Nth::First => Some((1, CalRRuleSide::Start)),
                            Nth::Second => Some((2, CalRRuleSide::Start)),
                            Nth::Third => Some((3, CalRRuleSide::Start)),
                            Nth::Last => Some((1, CalRRuleSide::End)),
                        };
                        rrule.set_by_day(Some(vec![CalWDayDesc::new(
                            self.monthly_wday.unwrap().into(),
                            nth,
                        )]));
                    }
                }
                _ => {}
            }

            match self.end {
                RecurEnd::Count => rrule.set_count(self.count),
                RecurEnd::Until => {
                    if let Some(ref until) = self.until {
                        rrule.set_until(
                            until
                                .to_caldate(CalDateType::Inclusive, false)
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

    /// Validates the recurrence `until` date against the given local timezone for DST safety.
    ///
    /// Returns `Ok(())` when the date is valid or absent. Returns the offending [`ParseError`]
    /// when the date falls in a DST gap or fold.
    pub fn check_dst(&self, local_tz: &Tz) -> Result<(), ParseError> {
        if self.end == RecurEnd::Until
            && let Some(cal_date) = self
                .until
                .as_ref()
                .and_then(|d| d.to_caldate(CalDateType::Inclusive, false))
        {
            cal_date.validate(local_tz)?;
        }
        Ok(())
    }
}

#[derive(Template)]
#[template(path = "comps/recur.htm")]
pub struct RecurTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    name: &'a str,
    id: String,
    freq: String,
    count: u64,
    interval: u8,
    end: RecurEnd,
    until: DateTemplate,
    weekly_days: String,
    monthly_type: MonthlyType,
    monthly_wday: ComboboxTemplate<IterWeekday>,
    monthly_nth: ComboboxTemplate<Nth>,
}

impl<'a> RecurTemplate<'a> {
    pub fn new(locale: Arc<dyn Locale + Send + Sync>, name: &'a str, value: RecurRequest) -> Self {
        Self {
            name,
            id: name.replace("[", "_").replace("]", "_"),
            freq: match value.freq {
                Some(f) => format!("{f}"),
                None => String::from("NONE"),
            },
            count: value.count,
            interval: value.interval,
            end: value.end,
            until: DateTemplate::new(format!("{name}[until]"), value.until),
            weekly_days: value.weekly_days,
            monthly_nth: ComboboxTemplate::new(
                locale.clone(),
                format!("{name}[monthly_nth]"),
                value.monthly_nth,
            ),
            monthly_type: value.monthly_type,
            monthly_wday: ComboboxTemplate::new(
                locale.clone(),
                format!("{name}[monthly_wday]"),
                value.monthly_wday,
            ),
            locale,
        }
    }
}
