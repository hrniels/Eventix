// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use chrono::offset::MappedLocalTime;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::objects::{
    CalDate, CalDateTime, CalDateType, CalRRule, CalRRuleSide, CalTimeZone, CalWDayDesc, Calendar,
    ResolvedDateTime,
};
use crate::parser::ParseError;
use crate::util;

/// Resolves calendar dates and datetimes using embedded `VTIMEZONE` data when available.
///
/// This type is the boundary between unresolved calendar values and concrete instants. It prefers
/// valid embedded timezone definitions from the calendar itself and falls back to the system
/// timezone database only when no usable embedded definition exists for a TZID.
#[derive(Clone, Debug)]
pub struct CalendarTimeZoneResolver {
    embedded: HashMap<String, EmbeddedTimeZone>,
}

impl Default for CalendarTimeZoneResolver {
    fn default() -> Self {
        Self {
            embedded: HashMap::new(),
        }
    }
}

impl CalendarTimeZoneResolver {
    /// Builds a resolver for the given calendar.
    ///
    /// Embedded `VTIMEZONE` definitions are compiled once and cached in the returned resolver.
    /// When a timezone is not available as an embedded definition, resolution falls back to the
    /// system `chrono_tz` database.
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

    /// Resolves the start instant represented by the given calendar date.
    ///
    /// `DATE` values are interpreted at local midnight in the fallback timezone. `DATE-TIME`
    /// values are resolved according to their own timezone semantics, using the fallback timezone
    /// for floating values.
    pub fn resolve_date_start(&self, date: &CalDate, fallback: &Tz) -> ResolvedDateTime {
        match date {
            CalDate::Date(day, _) => {
                fixed_from_fallback(fallback, day.and_hms_opt(0, 0, 0).unwrap())
            }
            CalDate::DateTime(dt) => self.resolve_datetime(dt, fallback),
        }
    }

    /// Resolves the end instant represented by the given calendar date.
    ///
    /// Exclusive `DATE` values resolve to the last second of the previous day, inclusive `DATE`
    /// values resolve to `23:59:59`, and `DATE-TIME` values are resolved according to their own
    /// timezone semantics.
    pub fn resolve_date_end(&self, date: &CalDate, fallback: &Tz) -> ResolvedDateTime {
        match date {
            CalDate::Date(day, CalDateType::Exclusive) => {
                let next_day = fixed_from_fallback(fallback, day.and_hms_opt(0, 0, 0).unwrap());
                next_day - Duration::seconds(1)
            }
            CalDate::Date(day, CalDateType::Inclusive) => {
                fixed_from_fallback(fallback, day.and_hms_opt(23, 59, 59).unwrap())
            }
            CalDate::DateTime(dt) => self.resolve_datetime(dt, fallback),
        }
    }

    /// Resolves a calendar datetime into a concrete instant.
    ///
    /// UTC values keep their original instant, floating values are interpreted in the fallback
    /// timezone, and `TZID` values are resolved against embedded `VTIMEZONE` data or the system
    /// timezone database when no embedded definition exists.
    pub fn resolve_datetime(&self, dt: &CalDateTime, fallback: &Tz) -> ResolvedDateTime {
        match dt {
            CalDateTime::Utc(dt) => dt.fixed_offset().into(),
            CalDateTime::Floating(local) => fixed_from_fallback(fallback, *local),
            CalDateTime::Timezone(local, tzid) => self.resolve_local_or_earlier(tzid, *local),
        }
    }

    fn resolve_local_or_earlier(&self, tzid: &str, local: NaiveDateTime) -> ResolvedDateTime {
        match self.resolve_local(tzid, local) {
            MappedLocalTime::Single(dt) => dt,
            MappedLocalTime::Ambiguous(early, _) => early,
            MappedLocalTime::None => panic!("non-existent local time {local} in {tzid}"),
        }
    }

    fn resolve_local(&self, tzid: &str, local: NaiveDateTime) -> MappedLocalTime<ResolvedDateTime> {
        if let Some(tz) = self.embedded.get(tzid) {
            return tz.resolve_local(local);
        }

        if let Ok(tz) = tzid.parse::<Tz>() {
            return map_system_time(tz, local);
        }

        // fall back to UTC as a last resort
        map_system_time(Tz::UTC, local)
    }

    /// Validates that the given calendar datetime is representable in both its declared timezone
    /// semantics and the caller's local timezone.
    ///
    /// This rejects datetimes that fall into DST gaps or folds for floating values, and for
    /// timezone-qualified values it additionally rejects invalid local times in the declared TZID.
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
                Ok(())
            }
        }
    }

    /// Validates that the given calendar date is representable in the caller's local timezone.
    ///
    /// `DATE` values validate both their start and end-of-day boundaries. `DATE-TIME` values are
    /// delegated to [`Self::validate_datetime`].
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

    fn pseudo_local(&self, dt: &CalDateTime, fallback: &Tz) -> DateTime<Utc> {
        match dt {
            CalDateTime::Utc(dt) => *dt,
            CalDateTime::Floating(local) | CalDateTime::Timezone(local, _) => {
                let _ = fallback;
                local.and_utc()
            }
        }
    }

    /// Converts the start of a calendar date into a pseudo-local recurrence seed.
    ///
    /// The returned `DateTime<Utc>` does not represent a real UTC instant. It is a timezone-neutral
    /// carrier of local wall-clock fields used by the recurrence engine before a later resolution
    /// step applies timezone rules.
    pub fn pseudo_local_date_start(&self, date: &CalDate, fallback: &Tz) -> DateTime<Utc> {
        match date {
            CalDate::Date(day, _) => {
                let _ = fallback;
                day.and_hms_opt(0, 0, 0).unwrap().and_utc()
            }
            CalDate::DateTime(dt) => self.pseudo_local(dt, fallback),
        }
    }

    /// Resolves a pseudo-local recurrence datetime into a concrete instant.
    ///
    /// This is the inverse of the pseudo-local recurrence carrier used in `recur.rs`: the date and
    /// time fields are interpreted as local wall-clock values and resolved through the given TZID or
    /// fallback timezone.
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

    /// Converts a concrete instant back into a local wall-clock datetime in the requested timezone.
    pub fn instant_to_local(
        &self,
        instant: ResolvedDateTime,
        tzid: Option<&str>,
        fallback: &Tz,
    ) -> NaiveDateTime {
        match tzid {
            Some(tzid) => self.localize_in_timezone(instant, tzid),
            None => instant.with_timezone(fallback).naive_local(),
        }
    }

    fn localize_in_timezone(&self, instant: ResolvedDateTime, tzid: &str) -> NaiveDateTime {
        if let Some(tz) = self.embedded.get(tzid) {
            // Embedded VTIMEZONE data is compiled into our own transition table, so chrono_tz
            // cannot do this conversion for us. Reconstruct the matching local wall-clock value
            // using the embedded offset history.
            return tz.localize_instant(instant);
        }

        if let Ok(tz) = tzid.parse::<Tz>() {
            return instant.with_timezone(&tz).naive_local();
        }

        instant.with_timezone(&Tz::UTC).naive_local()
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
                // Some embedded definitions only provide a fixed offset without explicit recurring
                // transition starts. Keep that as the fallback base offset for times outside the
                // generated transition window.
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

        // Start with the best known offset before this local time, then walk nearby transitions to
        // detect gaps/folds and update the active offset when a transition has already started.
        let mut active_offset = base_offset;
        let upper_bound = self
            .transitions
            .partition_point(|transition| transition.local_start <= local + Duration::hours(3));
        for transition in &self.transitions[..upper_bound] {
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
                    // During a fold both the pre-transition and post-transition offsets map the same
                    // wall-clock value to distinct instants, so return both possibilities.
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
                active_offset = FixedOffset::east_opt(transition.offset_to.as_seconds()).unwrap();
            }
        }

        MappedLocalTime::Single(fixed_datetime(local, active_offset).into())
    }

    fn base_offset_before(&self, local: NaiveDateTime) -> Option<FixedOffset> {
        if let Some(first) = self.transitions.first() {
            if local < first.local_start {
                return FixedOffset::east_opt(first.offset_from.as_seconds());
            }
        }

        // Prefer the most recent generated transition before this local time. If there is none,
        // fall back to a fixed observance when the embedded timezone did not yield recurring
        // transition starts.
        let transition_idx = self
            .transitions
            .partition_point(|transition| transition.local_start <= local);

        self.transitions
            .get(transition_idx.checked_sub(1)?)
            .and_then(|transition| FixedOffset::east_opt(transition.offset_to.as_seconds()))
            .or_else(|| {
                self.base_observance
                    .as_ref()
                    .and_then(|obs| FixedOffset::east_opt(obs.offset_to.as_seconds()))
            })
    }

    fn localize_instant(&self, instant: ResolvedDateTime) -> NaiveDateTime {
        let utc = instant.with_timezone(&Utc).naive_utc();
        let mut best_local = None;
        let mut best_seconds = i64::MIN;

        for offset in self.all_offsets() {
            // For a concrete instant, each distinct UTC offset implies exactly one possible local
            // wall-clock value: local = utc + offset. We do not know ahead of time which embedded
            // observance was active, so try every offset that appears in the compiled VTIMEZONE.
            let local = utc + Duration::seconds(i64::from(offset.local_minus_utc()));
            match self.resolve_local(local) {
                // Found the local wall-clock value whose forward resolution maps back to the same
                // instant. This is the inverse we were looking for.
                MappedLocalTime::Single(candidate) if candidate == instant => return local,
                MappedLocalTime::Ambiguous(early, late) if early == instant || late == instant => {
                    return local;
                }
                _ => {
                    // Keep the largest offset as a last-resort fallback. This should not normally
                    // be needed for well-formed data, but it avoids producing a wildly unrelated
                    // wall-clock time if the embedded rules are incomplete.
                    let seconds = i64::from(offset.local_minus_utc());
                    if seconds > best_seconds {
                        best_seconds = seconds;
                        best_local = Some(local);
                    }
                }
            }
        }

        best_local.unwrap_or(utc)
    }

    fn all_offsets(&self) -> Vec<FixedOffset> {
        let mut offsets: Vec<FixedOffset> = Vec::new();

        let mut push_unique_offset = |offset: FixedOffset| {
            if offsets
                .iter()
                .all(|existing| existing.local_minus_utc() != offset.local_minus_utc())
            {
                offsets.push(offset);
            }
        };

        // Collect every distinct offset that can appear in this embedded timezone. Reverse
        // localization only needs the set of offsets, not the full transition sequence.
        if let Some(base) = &self.base_observance {
            push_unique_offset(FixedOffset::east_opt(base.offset_from.as_seconds()).unwrap());
            push_unique_offset(FixedOffset::east_opt(base.offset_to.as_seconds()).unwrap());
        }

        for transition in &self.transitions {
            push_unique_offset(FixedOffset::east_opt(transition.offset_from.as_seconds()).unwrap());
            push_unique_offset(FixedOffset::east_opt(transition.offset_to.as_seconds()).unwrap());
        }

        offsets
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
            // Embedded observance rules are expanded as local wall-clock transition starts. The
            // resolver later interprets these starts together with offset_from/offset_to to detect
            // gaps and folds.
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
