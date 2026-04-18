// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use chrono::offset::MappedLocalTime;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::objects::{
    CalDate, CalDateTime, CalRRule, CalRRuleSide, CalTimeZone, CalWDayDesc, Calendar,
    ResolvedDateTime,
};
use crate::parser::ParseError;
use crate::util;

#[derive(Clone, Debug)]
pub struct CalendarTimeZoneResolver {
    embedded: HashMap<String, EmbeddedTimeZone>,
}

impl CalendarTimeZoneResolver {
    pub fn new(calendar: &Calendar) -> Self {
        let embedded = calendar
            .timezones()
            .iter()
            .filter_map(|tz| {
                EmbeddedTimeZone::compile(tz).map(|compiled| (tz.tzid().to_string(), compiled))
            })
            .collect();
        Self { embedded }
    }

    pub fn has_embedded_timezone(&self, tzid: &str) -> bool {
        self.embedded.contains_key(tzid)
    }

    pub fn resolve_local(
        &self,
        tzid: &str,
        local: NaiveDateTime,
    ) -> MappedLocalTime<ResolvedDateTime> {
        if let Some(tz) = self.embedded.get(tzid) {
            return tz.resolve_local(local);
        }

        if let Ok(tz) = tzid.parse::<Tz>() {
            return map_system_time(tz, local);
        }

        map_system_time(Tz::Europe__Berlin, local)
    }

    pub fn resolve_local_or_earlier(&self, tzid: &str, local: NaiveDateTime) -> ResolvedDateTime {
        match self.resolve_local(tzid, local) {
            MappedLocalTime::Single(dt) => dt,
            MappedLocalTime::Ambiguous(early, _) => early,
            MappedLocalTime::None => panic!("non-existent local time {local} in {tzid}"),
        }
    }

    pub fn resolve_date_start(&self, date: &CalDate, fallback: &Tz) -> ResolvedDateTime {
        match date {
            CalDate::Date(day, _) => {
                fixed_from_fallback(fallback, day.and_hms_opt(0, 0, 0).unwrap())
            }
            CalDate::DateTime(dt) => self.resolve_datetime(dt, fallback),
        }
    }

    pub fn resolve_date_end(&self, date: &CalDate, fallback: &Tz) -> ResolvedDateTime {
        match date {
            CalDate::Date(day, crate::objects::CalDateType::Exclusive) => {
                let next_day = fixed_from_fallback(fallback, day.and_hms_opt(0, 0, 0).unwrap());
                next_day - Duration::seconds(1)
            }
            CalDate::Date(day, crate::objects::CalDateType::Inclusive) => {
                fixed_from_fallback(fallback, day.and_hms_opt(23, 59, 59).unwrap())
            }
            CalDate::DateTime(dt) => self.resolve_datetime(dt, fallback),
        }
    }

    pub fn resolve_datetime(&self, dt: &CalDateTime, fallback: &Tz) -> ResolvedDateTime {
        match dt {
            CalDateTime::Utc(dt) => dt.fixed_offset().into(),
            CalDateTime::Floating(local) => fixed_from_fallback(fallback, *local),
            CalDateTime::Timezone(local, tzid) => self.resolve_local_or_earlier(tzid, *local),
        }
    }

    pub fn validate_datetime(&self, dt: &CalDateTime, local_tz: &Tz) -> Result<(), ParseError> {
        match dt {
            CalDateTime::Utc(_) => Ok(()),
            CalDateTime::Floating(local) => validate_system_time(local_tz, *local),
            CalDateTime::Timezone(local, tzid) => {
                match self.resolve_local(tzid, *local) {
                    MappedLocalTime::None => {
                        return Err(ParseError::NonExistentTime(format!(
                            "{} in {}",
                            local, tzid
                        )));
                    }
                    MappedLocalTime::Ambiguous(_, _) => {
                        return Err(ParseError::AmbiguousTime(format!("{} in {}", local, tzid)));
                    }
                    MappedLocalTime::Single(_) => {}
                }
                validate_system_time(local_tz, *local)
            }
        }
    }

    pub fn validate_date(&self, date: &CalDate, local_tz: &Tz) -> Result<(), ParseError> {
        match date {
            CalDate::Date(day, _) => {
                validate_system_time(local_tz, day.and_hms_opt(0, 0, 0).unwrap())?;
                validate_system_time(local_tz, day.and_hms_opt(23, 59, 59).unwrap())?;
                Ok(())
            }
            CalDate::DateTime(dt) => self.validate_datetime(dt, local_tz),
        }
    }

    pub fn pseudo_local(&self, dt: &CalDateTime, fallback: &Tz) -> DateTime<Utc> {
        match dt {
            CalDateTime::Utc(dt) => *dt,
            CalDateTime::Floating(local) | CalDateTime::Timezone(local, _) => {
                let _ = fallback;
                local.and_utc()
            }
        }
    }

    pub fn pseudo_local_date_start(&self, date: &CalDate, fallback: &Tz) -> DateTime<Utc> {
        match date {
            CalDate::Date(day, _) => {
                let _ = fallback;
                day.and_hms_opt(0, 0, 0).unwrap().and_utc()
            }
            CalDate::DateTime(dt) => self.pseudo_local(dt, fallback),
        }
    }

    pub fn pseudo_local_date_end(&self, date: &CalDate, fallback: &Tz) -> DateTime<Utc> {
        match date {
            CalDate::Date(day, crate::objects::CalDateType::Exclusive) => {
                let _ = fallback;
                day.and_hms_opt(0, 0, 0).unwrap().and_utc() - Duration::seconds(1)
            }
            CalDate::Date(day, crate::objects::CalDateType::Inclusive) => {
                let _ = fallback;
                day.and_hms_opt(23, 59, 59).unwrap().and_utc()
            }
            CalDate::DateTime(dt) => self.pseudo_local(dt, fallback),
        }
    }

    pub fn resolve_pseudo_local(
        &self,
        pseudo: DateTime<Utc>,
        tzid: Option<&str>,
        fallback: &Tz,
    ) -> ResolvedDateTime {
        let local = pseudo.naive_utc();
        match tzid {
            Some(tzid) => self.resolve_local_or_earlier(tzid, local),
            None => fixed_from_fallback(fallback, local),
        }
    }

    pub fn fixed_to_tz(dt: ResolvedDateTime, tz: &Tz) -> chrono::DateTime<Tz> {
        dt.with_timezone(tz)
    }
}

fn validate_system_time(tz: &Tz, local: NaiveDateTime) -> Result<(), ParseError> {
    match tz.from_local_datetime(&local) {
        MappedLocalTime::None => Err(ParseError::NonExistentTime(format!("{} in {}", local, tz))),
        MappedLocalTime::Ambiguous(_, _) => {
            Err(ParseError::AmbiguousTime(format!("{} in {}", local, tz)))
        }
        MappedLocalTime::Single(_) => Ok(()),
    }
}

fn fixed_from_fallback(tz: &Tz, local: NaiveDateTime) -> ResolvedDateTime {
    match tz.from_local_datetime(&local) {
        MappedLocalTime::Single(dt) => dt.fixed_offset().into(),
        MappedLocalTime::Ambiguous(early, _) => early.fixed_offset().into(),
        MappedLocalTime::None => panic!("non-existent local time {local} in {tz}"),
    }
}

fn map_system_time(tz: Tz, local: NaiveDateTime) -> MappedLocalTime<ResolvedDateTime> {
    match tz.from_local_datetime(&local) {
        MappedLocalTime::Single(dt) => MappedLocalTime::Single(dt.fixed_offset().into()),
        MappedLocalTime::Ambiguous(early, late) => {
            MappedLocalTime::Ambiguous(early.fixed_offset().into(), late.fixed_offset().into())
        }
        MappedLocalTime::None => MappedLocalTime::None,
    }
}

#[derive(Clone, Debug)]
struct EmbeddedTimeZone {
    transitions: Vec<Transition>,
    base_observance: Option<FixedObservance>,
}

impl EmbeddedTimeZone {
    fn compile(timezone: &CalTimeZone) -> Option<Self> {
        let mut transitions = Vec::new();
        let mut base_observance = None;

        for observance in timezone.observances() {
            let fixed = FixedObservance::compile(observance)?;
            let starts = fixed.transition_starts(1970, 2100);
            if starts.is_empty() {
                base_observance = Some(fixed.clone());
            }
            transitions.extend(starts.into_iter().map(|start| Transition {
                local_start: start,
                offset_from: fixed.offset_from,
                offset_to: fixed.offset_to,
            }));
        }

        transitions.sort_by_key(|t| t.local_start);
        Some(Self {
            transitions,
            base_observance,
        })
    }

    fn resolve_local(&self, local: NaiveDateTime) -> MappedLocalTime<ResolvedDateTime> {
        let Some(base_offset) = self.base_offset_before(local) else {
            return MappedLocalTime::None;
        };

        let mut candidates = vec![base_offset];
        for transition in &self.transitions {
            if transition.local_start > local + Duration::hours(3) {
                break;
            }

            let gap = transition.offset_to.as_seconds() - transition.offset_from.as_seconds();
            if gap > 0 {
                let gap_end = transition.local_start + Duration::seconds(gap as i64);
                if local >= transition.local_start && local < gap_end {
                    return MappedLocalTime::None;
                }
            } else if gap < 0 {
                let overlap_start = transition.local_start + Duration::seconds(gap as i64);
                if local >= overlap_start && local < transition.local_start {
                    let early = fixed_datetime(
                        local,
                        FixedOffset::east_opt(transition.offset_from.as_seconds()).unwrap(),
                    );
                    let late = fixed_datetime(
                        local,
                        FixedOffset::east_opt(transition.offset_to.as_seconds()).unwrap(),
                    );
                    return MappedLocalTime::Ambiguous(early.into(), late.into());
                }
            }

            if transition.local_start <= local {
                candidates.push(FixedOffset::east_opt(transition.offset_to.as_seconds()).unwrap());
            }
        }

        MappedLocalTime::Single(fixed_datetime(local, *candidates.last().unwrap()).into())
    }

    fn base_offset_before(&self, local: NaiveDateTime) -> Option<FixedOffset> {
        if let Some(first) = self.transitions.first() {
            if local < first.local_start {
                return FixedOffset::east_opt(first.offset_from.as_seconds());
            }
        }

        self.transitions
            .iter()
            .rev()
            .find(|t| t.local_start <= local)
            .and_then(|t| FixedOffset::east_opt(t.offset_to.as_seconds()))
            .or_else(|| {
                self.base_observance
                    .as_ref()
                    .and_then(|obs| FixedOffset::east_opt(obs.offset_to.as_seconds()))
            })
    }
}

#[derive(Clone, Debug)]
struct Transition {
    local_start: NaiveDateTime,
    offset_from: crate::objects::CalUtcOffset,
    offset_to: crate::objects::CalUtcOffset,
}

#[derive(Clone, Debug)]
struct FixedObservance {
    dtstart: NaiveDateTime,
    offset_from: crate::objects::CalUtcOffset,
    offset_to: crate::objects::CalUtcOffset,
    rrule: Option<CalRRule>,
    rdate: Vec<NaiveDateTime>,
}

impl FixedObservance {
    fn compile(observance: &crate::objects::CalTimeZoneObservance) -> Option<Self> {
        let dtstart = match observance.dtstart() {
            CalDateTime::Floating(dt) => *dt,
            _ => return None,
        };
        let rdate = observance
            .rdate()
            .iter()
            .filter_map(|d| match d {
                CalDateTime::Floating(dt) => Some(*dt),
                _ => None,
            })
            .collect();
        Some(Self {
            dtstart,
            offset_from: observance.tzoffset_from(),
            offset_to: observance.tzoffset_to(),
            rrule: observance.rrule().cloned(),
            rdate,
        })
    }

    fn transition_starts(&self, start_year: i32, end_year: i32) -> Vec<NaiveDateTime> {
        let mut starts = vec![self.dtstart];
        starts.extend(self.rdate.iter().copied());

        if let Some(rrule) = &self.rrule {
            for year in start_year..=end_year {
                starts.extend(expand_observance_rrule(self.dtstart, rrule, year));
            }
        }

        starts.sort();
        starts.dedup();
        starts
    }
}

fn fixed_datetime(local: NaiveDateTime, offset: FixedOffset) -> DateTime<FixedOffset> {
    offset.from_local_datetime(&local).single().unwrap()
}

fn expand_observance_rrule(
    dtstart: NaiveDateTime,
    rrule: &CalRRule,
    year: i32,
) -> Vec<NaiveDateTime> {
    let months: Vec<u32> = rrule
        .by_month()
        .cloned()
        .unwrap_or_else(|| vec![dtstart.month() as u8])
        .into_iter()
        .map(u32::from)
        .collect();

    let mut dates = Vec::new();
    for month in months {
        if let Some(by_day) = rrule.by_day() {
            for desc in by_day {
                if let Some(day) = resolve_month_weekday(year, month, desc) {
                    dates.push(day.and_time(dtstart.time()));
                }
            }
        } else if let Some(by_mday) = rrule.by_mon_day() {
            for desc in by_mday {
                let days = util::month_days(year, month);
                let dom = match desc.side() {
                    CalRRuleSide::Start => desc.num() as u32,
                    CalRRuleSide::End => days - (desc.num() - 1) as u32,
                };
                if let Some(day) = NaiveDate::from_ymd_opt(year, month, dom) {
                    dates.push(day.and_time(dtstart.time()));
                }
            }
        } else if let Some(day) = NaiveDate::from_ymd_opt(year, month, dtstart.day()) {
            dates.push(day.and_time(dtstart.time()));
        }
    }

    dates
}

fn resolve_month_weekday(year: i32, month: u32, desc: &CalWDayDesc) -> Option<NaiveDate> {
    match desc.nth() {
        Some((nth, CalRRuleSide::Start)) => {
            NaiveDate::from_weekday_of_month_opt(year, month, desc.day(), nth)
        }
        Some((nth, CalRRuleSide::End)) => {
            let (n_year, n_month) = util::next_month(year, month);
            let next_month = NaiveDate::from_ymd_opt(n_year, n_month, 1)?;
            let last = next_month.pred_opt()?;
            let last_weekday = last.weekday();
            let first_to_dow =
                (7 + last_weekday.number_from_monday() - desc.day().number_from_monday()) % 7;
            let day = last.day() - ((nth - 1) as u32 * 7 + first_to_dow);
            NaiveDate::from_ymd_opt(year, month, day)
        }
        None => {
            let first = NaiveDate::from_ymd_opt(year, month, 1)?;
            let delta =
                (7 + desc.day().number_from_monday() - first.weekday().number_from_monday()) % 7;
            NaiveDate::from_ymd_opt(year, month, 1 + delta)
        }
    }
}
