// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{
    DateTime, Datelike, Duration, Month, Months, NaiveDate, NaiveDateTime, TimeDelta, TimeZone,
    Timelike, Utc, Weekday,
};
use chrono_tz::Tz;
use formatx::formatx;
use itertools::Itertools;
use std::fmt;
use std::str::FromStr;

use crate::objects::{CalDate, CalLocale, DateContext};
use crate::parser::{ParseError, Property};
use crate::util;

/// The frequency for recurrences.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CalRRuleFreq {
    Secondly,
    Minutely,
    Hourly,
    Daily,
    #[default]
    Weekly,
    Monthly,
    Yearly,
}

impl CalRRuleFreq {
    /// Advances the given date based on this frequency and given interval.
    ///
    /// For example, if `self` is [`Self::Daily`], the given date will be advanced by `interval`
    /// days forward.
    pub fn advance(&self, now: NaiveDateTime, interval: u32) -> Option<NaiveDateTime> {
        match self {
            Self::Secondly => now.checked_add_signed(TimeDelta::seconds(interval.into())),
            Self::Minutely => now.checked_add_signed(TimeDelta::minutes(interval.into())),
            Self::Hourly => now.checked_add_signed(TimeDelta::hours(interval.into())),
            Self::Daily => now.checked_add_signed(TimeDelta::days(interval.into())),
            Self::Weekly => now.checked_add_signed(TimeDelta::weeks(interval.into())),
            Self::Monthly => now.checked_add_months(Months::new(interval)),
            Self::Yearly => now.checked_add_months(Months::new(interval * 12)),
        }
    }
}

impl fmt::Display for CalRRuleFreq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Secondly => "SECONDLY",
            Self::Minutely => "MINUTELY",
            Self::Hourly => "HOURLY",
            Self::Daily => "DAILY",
            Self::Weekly => "WEEKLY",
            Self::Monthly => "MONTHLY",
            Self::Yearly => "YEARLY",
        };
        write!(f, "{name}")
    }
}

impl FromStr for CalRRuleFreq {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SECONDLY" => Ok(Self::Secondly),
            "MINUTELY" => Ok(Self::Minutely),
            "HOURLY" => Ok(Self::Hourly),
            "DAILY" => Ok(Self::Daily),
            "WEEKLY" => Ok(Self::Weekly),
            "MONTHLY" => Ok(Self::Monthly),
            "YEARLY" => Ok(Self::Yearly),
            _ => Err(ParseError::InvalidFrequency(s.to_string())),
        }
    }
}

/// The "side" for start/end relative repetitions.
///
/// For example, the second to last Tuesday in a month is [`End`](Self::End) as it is relative to
/// the end of the month.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CalRRuleSide {
    /// Relative to the start of the month/year.
    Start,

    /// Relative to the end of the month/year.
    End,
}

impl FromStr for CalRRuleSide {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.as_bytes()[0] {
            b'+' => Ok(Self::Start),
            b'-' => Ok(Self::End),
            _ => Err(ParseError::InvalidSide(s.to_string())),
        }
    }
}

/// Represents a weekday repetition.
///
/// For example, this allows to specify a repetition of an event on every Wednesday or on every
/// third Saturday of the month.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct CalWDayDesc {
    day: Weekday,
    nth: Option<(u8, CalRRuleSide)>,
}

impl CalWDayDesc {
    /// Parses the given weekday name into a [`Weekday`].
    ///
    /// The weekday is expected to be the first two letters in uppercase.
    pub fn parse_weekday(s: &str) -> Result<Weekday, ParseError> {
        match s {
            "SU" => Ok(Weekday::Sun),
            "MO" => Ok(Weekday::Mon),
            "TU" => Ok(Weekday::Tue),
            "WE" => Ok(Weekday::Wed),
            "TH" => Ok(Weekday::Thu),
            "FR" => Ok(Weekday::Fri),
            "SA" => Ok(Weekday::Sat),
            _ => Err(ParseError::InvalidWeekday(s.to_string())),
        }
    }

    /// The string representation of the given [`Weekday`].
    ///
    /// The string representation uses the first two letters in uppercase.
    pub fn to_weekday_str(wday: Weekday) -> &'static str {
        match wday {
            Weekday::Mon => "MO",
            Weekday::Tue => "TU",
            Weekday::Wed => "WE",
            Weekday::Thu => "TH",
            Weekday::Fri => "FR",
            Weekday::Sat => "SA",
            Weekday::Sun => "SU",
        }
    }

    /// Creates a new instance of [`CalWDayDesc`].
    ///
    /// The `day` specifies the weekday, whereas `nth` optionally describes the specific instance
    /// of that weekday starting at either the start or end of the month/year.
    ///
    /// For example, `CalWDayDesc::new(Weekday::Tue, Some((2, CalRRuleSide::Start)))` creates a
    /// repetition on every second Tuesday from the start of the month/year.
    pub fn new(day: Weekday, nth: Option<(u8, CalRRuleSide)>) -> Self {
        Self { day, nth }
    }

    /// The weekday on which it occurs.
    pub fn day(&self) -> Weekday {
        self.day
    }

    /// The nth instance of that weekday.
    ///
    /// This optionally describes the specific instance of the weekday starting at either the
    /// start or end of the month/year.
    pub fn nth(&self) -> Option<(u8, CalRRuleSide)> {
        self.nth
    }

    /// Returns true if the date matches this weekday repetition for the given recurrence rule.
    ///
    /// For example, if `rrule` repeats monthly and `self` specifies that it occurs on every second
    /// Wednesday, this method returns true if `date` is the second Wednesday of any month.
    pub fn matches(&self, date: DateTime<Utc>, rrule: &CalRRule) -> bool {
        match self.nth {
            None => self.day == date.weekday(),
            Some((n, side)) => {
                // offset within the month
                if rrule.freq == CalRRuleFreq::Monthly
                    || (rrule.freq == CalRRuleFreq::Yearly && rrule.by_month.is_some())
                {
                    match side {
                        CalRRuleSide::Start => NaiveDate::from_weekday_of_month_opt(
                            date.year(),
                            date.month(),
                            self.day,
                            n,
                        ),
                        CalRRuleSide::End => {
                            let (year, month) = util::next_month(date.year(), date.month());
                            let next_month = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
                            let last = next_month.pred_opt().unwrap();
                            let last_weekday = last.weekday();
                            let first_to_dow = (7 + last_weekday.number_from_monday()
                                - self.day.number_from_monday())
                                % 7;
                            let day = last.day() - ((n - 1) as u32 * 7 + first_to_dow);
                            NaiveDate::from_ymd_opt(date.year(), date.month(), day)
                        }
                    }
                    .map(|d| d == date.date_naive())
                    .unwrap_or(false)
                }
                // offset within the year
                else if rrule.freq == CalRRuleFreq::Yearly {
                    match side {
                        CalRRuleSide::Start => {
                            let year_start = NaiveDate::from_ymd_opt(date.year(), 1, 1).unwrap();
                            let first_weekday = year_start.weekday();
                            let first_to_dow = (7 + self.day.number_from_monday()
                                - first_weekday.number_from_monday())
                                % 7;
                            Some(
                                year_start
                                    + Duration::days(((n - 1) as u32 * 7 + first_to_dow) as i64),
                            )
                        }
                        CalRRuleSide::End => {
                            let year_end = NaiveDate::from_ymd_opt(date.year(), 12, 31).unwrap();
                            let last_weekday = year_end.weekday();
                            let first_to_dow = (7 + last_weekday.number_from_monday()
                                - self.day.number_from_monday())
                                % 7;
                            Some(
                                year_end
                                    - Duration::days(((n - 1) as u32 * 7 + first_to_dow) as i64),
                            )
                        }
                    }
                    .map(|d| d == date.date_naive())
                    .unwrap_or(false)
                } else if rrule.freq == CalRRuleFreq::Weekly {
                    self.day == date.weekday()
                } else {
                    // anything else is invalid
                    false
                }
            }
        }
    }

    /// Returns a human-readable representation of this description.
    pub fn human<'l>(&self, locale: &'l dyn CalLocale) -> WeekdayHuman<'_, 'l> {
        WeekdayHuman { wday: self, locale }
    }
}

impl FromStr for CalWDayDesc {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (s, side) = if s.starts_with('-') || s.starts_with('+') {
            (&s[1..], s.parse::<CalRRuleSide>()?)
        } else {
            (s, CalRRuleSide::Start)
        };

        if s.is_empty() {
            return Err(ParseError::UnexpectedWDayEnd);
        }

        let mut rest = s;
        while rest.as_bytes()[0].is_ascii_digit() {
            rest = &rest[1..];
        }

        let (s, nth) = if rest.len() == s.len() {
            (s, None)
        } else {
            (
                rest,
                Some((s[0..s.len() - rest.len()].parse::<u8>()?, side)),
            )
        };

        let day = Self::parse_weekday(s)?;
        Ok(Self { day, nth })
    }
}

impl fmt::Display for CalWDayDesc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some((num, side)) = self.nth {
            match side {
                CalRRuleSide::Start => write!(f, "+")?,
                CalRRuleSide::End => write!(f, "-")?,
            }
            write!(f, "{num}")?;
        }

        write!(f, "{}", Self::to_weekday_str(self.day))
    }
}

/// Implements [`Display`](fmt::Display) to create a human-readable representation of a
/// [`CalWDayDesc`].
///
/// For example, it could say "3rd to last Wednesday".
pub struct WeekdayHuman<'a, 'l> {
    wday: &'a CalWDayDesc,
    locale: &'l dyn CalLocale,
}

impl fmt::Display for WeekdayHuman<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let wday = format!("{}", self.wday.day);
        let wday = self.locale.translate(&wday);
        if let Some((num, side)) = self.wday.nth {
            write!(
                f,
                "{} {}",
                self.locale.nth_day(num as u64, side == CalRRuleSide::Start),
                wday
            )
        } else {
            write!(f, "{}", wday)
        }
    }
}

/// Represents a day number within a month or year.
///
/// The `num` field holds a 1-based ordinal (e.g. 1 for the first day), and `side` indicates
/// whether the ordinal counts from the start or the end of the period.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct DayDesc {
    num: u16,
    side: CalRRuleSide,
}

impl DayDesc {
    /// Creates a new [`DayDesc`] with the given ordinal number and side.
    pub fn new(num: u16, side: CalRRuleSide) -> Self {
        Self { num, side }
    }

    /// Returns the ordinal number.
    pub fn num(&self) -> u16 {
        self.num
    }

    /// Returns the side (start or end of the period).
    pub fn side(&self) -> CalRRuleSide {
        self.side
    }

    pub fn human<'l>(&self, locale: &'l dyn CalLocale) -> DayDescHuman<'_, 'l> {
        DayDescHuman { day: self, locale }
    }
}

impl FromStr for DayDesc {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (s, side) = if s.starts_with('-') || s.starts_with('+') {
            (&s[1..], s.parse::<CalRRuleSide>()?)
        } else {
            (s, CalRRuleSide::Start)
        };
        let num = s.parse::<u16>()?;
        Ok(Self { num, side })
    }
}

impl fmt::Display for DayDesc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.side {
            CalRRuleSide::Start => write!(f, "{}", self.num),
            CalRRuleSide::End => write!(f, "-{}", self.num),
        }
    }
}

pub struct DayDescHuman<'d, 'l> {
    day: &'d DayDesc,
    locale: &'l dyn CalLocale,
}

impl fmt::Display for DayDescHuman<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.locale
                .nth_day(self.day.num as u64, self.day.side == CalRRuleSide::Start)
        )
    }
}

/// Represents a recurrence rule.
///
/// Each recurrence has at least a frequency (daily, weekly, ...) and optionally several other
/// properties that further restrict it or expand upon this. Furthermore, recurrences repeat by
/// default indefinitely and can optionally be restricted to repeat a certain number of times or
/// until a specific date.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.3.10>.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct CalRRule {
    freq: CalRRuleFreq,
    until: Option<CalDate>,
    count: Option<u64>,
    interval: Option<u8>,
    by_second: Option<Vec<u8>>,
    by_minute: Option<Vec<u8>>,
    by_hour: Option<Vec<u8>>,
    by_day: Option<Vec<CalWDayDesc>>,
    by_mon_day: Option<Vec<DayDesc>>,
    by_year_day: Option<Vec<DayDesc>>,
    by_week_no: Option<Vec<DayDesc>>,
    by_month: Option<Vec<u8>>,
    by_set_pos: Option<Vec<DayDesc>>,
    week_start: Option<Weekday>,
}

fn next_date(date: DateTime<Utc>, freq: CalRRuleFreq, interval: u32) -> Option<DateTime<Utc>> {
    freq.advance(date.naive_utc(), interval)
        .map(|next| next.and_utc())
}

fn week_start_for_year(year: i32, week_start: Weekday) -> NaiveDate {
    // RFC 5545 uses the ISO-like definition for week 1 (week containing Jan 4).
    // This keeps week numbering stable without pulling in a separate ISO-week dependency.
    let jan_fourth = NaiveDate::from_ymd_opt(year, 1, 4).unwrap();
    let diff = jan_fourth.weekday().days_since(week_start) as i64;
    jan_fourth - Duration::days(diff)
}

/// Converts ordinal day rules into concrete dates so later filters can stay uniform.
fn base_dates_from_by_year_day(date: DateTime<Utc>, by_year_day: &[DayDesc]) -> Vec<DateTime<Utc>> {
    let year = date.year();
    let days_in_year = util::year_days(year) as i32;
    let mut base_dates = Vec::new();

    for yd in by_year_day {
        if yd.num() == 0 {
            continue;
        }
        let ordinal = match yd.side() {
            CalRRuleSide::Start => yd.num() as i32,
            CalRRuleSide::End => days_in_year - (yd.num() as i32 - 1),
        };
        if ordinal < 1 || ordinal > days_in_year {
            continue;
        }
        if let Some(naive) = NaiveDate::from_yo_opt(year, ordinal as u32)
            && let Some(base) = date.with_month(naive.month())
            && let Some(base) = base.with_day(naive.day())
        {
            base_dates.push(base);
        }
    }

    base_dates
}

/// Expands week numbers into actual dates before applying BYDAY or time-of-day filters.
fn base_dates_from_by_week_no(
    date: DateTime<Utc>,
    by_week_no: &[DayDesc],
    week_start: Option<Weekday>,
) -> Vec<DateTime<Utc>> {
    let year = date.year();
    let wkst = week_start.unwrap_or(Weekday::Mon);
    let week1_start = week_start_for_year(year, wkst);
    let next_year_week1_start = week_start_for_year(year + 1, wkst);
    let last_week_start = next_year_week1_start - Duration::weeks(1);
    let mut base_dates = Vec::new();

    for wn in by_week_no {
        if wn.num() == 0 {
            continue;
        }
        let week_start = match wn.side() {
            CalRRuleSide::Start => week1_start + Duration::weeks(wn.num() as i64 - 1),
            CalRRuleSide::End => last_week_start - Duration::weeks(wn.num() as i64 - 1),
        };
        for day_offset in 0..7 {
            let naive = week_start + Duration::days(day_offset);
            if naive.year() != year {
                continue;
            }
            if let Some(base) = date.with_month(naive.month())
                && let Some(base) = base.with_day(naive.day())
            {
                base_dates.push(base);
            }
        }
    }

    base_dates
}

/// Keeps BYMONTH filtering isolated so year-based paths align with the main expansion flow.
fn passes_by_month(date: DateTime<Utc>, by_month: Option<&Vec<u8>>) -> bool {
    match by_month {
        Some(list) => list.contains(&(date.month() as u8)),
        None => true,
    }
}

/// Uses the same month-day semantics for year-based candidates as elsewhere.
fn passes_by_month_day(date: DateTime<Utc>, by_mon_day: Option<&Vec<DayDesc>>) -> bool {
    let Some(list) = by_mon_day else {
        return true;
    };
    list.iter().any(|md| match md.side() {
        CalRRuleSide::Start => md.num() as u32 == date.day(),
        CalRRuleSide::End => {
            let days = util::month_days(date.year(), date.month());
            days - (md.num() - 1) as u32 == date.day()
        }
    })
}

/// Centralizes weekday matching so BYWEEKNO/BYYEARDAY stay consistent with other paths.
fn passes_by_day(date: DateTime<Utc>, by_day: Option<&Vec<CalWDayDesc>>, rrule: &CalRRule) -> bool {
    match by_day {
        Some(list) => list.iter().any(|d| d.matches(date, rrule)),
        None => true,
    }
}

/// Iterator for [`CalRRule`].
pub struct RecurIterator<'a> {
    rrule: &'a CalRRule,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    dtstart: DateTime<Utc>,
    dtdur: Option<Duration>,
    date: DateTime<Utc>,
    until: DateTime<Utc>,
    count: usize,
    interval: u32,
    last: Vec<DateTime<Utc>>,
    last_pos: usize,
}

impl Iterator for RecurIterator<'_> {
    type Item = DateTime<Utc>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.last_pos >= self.last.len() {
            // if we've already reached the limit on the last expand call, stop here
            if let Some(rcount) = self.rrule.count
                && self.count >= rcount as usize
            {
                return None;
            }

            while self.date <= self.until {
                if !self.rrule.limited(self.date) {
                    match self.rrule.expand(
                        self.dtstart,
                        self.dtdur,
                        self.start,
                        self.end,
                        self.date,
                        &mut self.count,
                    ) {
                        Some(dates) => self.last = dates,
                        None => return None,
                    }

                    // if we've found something, walk through that
                    if !self.last.is_empty() {
                        self.last_pos = 0;
                        self.date = next_date(self.date, self.rrule.freq, self.interval).unwrap();
                        break;
                    }
                }
                self.date = next_date(self.date, self.rrule.freq, self.interval).unwrap();
            }
        }

        if self.last_pos >= self.last.len() {
            return None;
        }
        self.last_pos += 1;
        Some(self.last[self.last_pos - 1])
    }
}

impl CalRRule {
    /// Returns the frequency of this recurrence rule (FREQ).
    pub fn frequency(&self) -> CalRRuleFreq {
        self.freq
    }
    /// Sets the frequency of this recurrence rule (FREQ).
    pub fn set_frequency(&mut self, freq: CalRRuleFreq) {
        self.freq = freq;
    }

    /// Returns the date until the recurrence lasts (UNTIL).
    pub fn until(&self) -> Option<&CalDate> {
        self.until.as_ref()
    }
    /// Sets the date until the recurrence lasts (UNTIL).
    pub fn set_until(&mut self, until: CalDate) {
        self.until = Some(until);
    }

    /// Returns the number of recurrences (COUNT).
    ///
    /// If it is `None`, the recurrence occurs indefinitely.
    pub fn count(&self) -> Option<u64> {
        self.count
    }
    /// Sets the number of recurrences (COUNT).
    ///
    /// If set to `None`, the recurrence occurs indefinitely.
    pub fn set_count(&mut self, count: u64) {
        self.count = Some(count);
    }

    /// Returns the interval between recurrences (INTERVAL).
    ///
    /// For example, a frequency of daily and an interval of 4 leads to an recurrence every 4 days.
    pub fn interval(&self) -> Option<u8> {
        self.interval
    }
    /// Sets the interval between recurrences (INTERVAL).
    ///
    /// For example, a frequency of daily and an interval of 4 leads to an recurrence every 4 days.
    pub fn set_interval(&mut self, interval: u8) {
        self.interval = Some(interval);
    }

    /// Returns the by-day specification (BYDAY).
    ///
    /// The by-day specification is used to create recurrences on specific weekdays. For example,
    /// it can be used to create a recurrence on every 3rd Monday of each month. As a recurrence
    /// can also happen on multiple of such weekday descriptions, it is specified as a `Vec`.
    pub fn by_day(&self) -> Option<&Vec<CalWDayDesc>> {
        self.by_day.as_ref()
    }
    /// Sets the by-day specification (BYDAY).
    ///
    /// The by-day specification is used to create recurrences on specific weekdays. For example,
    /// it can be used to create a recurrence on every 3rd Monday of each month. As a recurrence
    /// can also happen on multiple of such weekday descriptions, it is specified as a `Vec`.
    pub fn set_by_day(&mut self, by_day: Option<Vec<CalWDayDesc>>) {
        self.by_day = by_day;
    }

    /// Returns the by-month-day specification (BYMONTHDAY).
    ///
    /// Each entry is a day-of-month ordinal that can count from the start or end of the month.
    /// For example, `DayDesc::new(1, CalRRuleSide::Start)` means the 1st of the month, and
    /// `DayDesc::new(1, CalRRuleSide::End)` means the last day.
    pub fn by_mon_day(&self) -> Option<&Vec<DayDesc>> {
        self.by_mon_day.as_ref()
    }

    /// Sets the by-month-day specification (BYMONTHDAY).
    pub fn set_by_mon_day(&mut self, by_mon_day: Option<Vec<DayDesc>>) {
        self.by_mon_day = by_mon_day;
    }

    /// Returns the by-month specification (BYMONTH).
    ///
    /// Each entry is a month number in the range 1–12 (January = 1, December = 12).
    pub fn by_month(&self) -> Option<&Vec<u8>> {
        self.by_month.as_ref()
    }

    /// Sets the by-month specification (BYMONTH).
    ///
    /// Each entry must be a month number in the range 1–12.
    pub fn set_by_month(&mut self, by_month: Option<Vec<u8>>) {
        self.by_month = by_month;
    }

    /// Returns an iterator with all recurrences between `start` and `end`.
    ///
    /// The recurrence starts with `dtstart` (DTSTART of the calendar component) and each has a
    /// duration of `dtdur` (DTDUR). `start` and `end` specify the time interval the caller is
    /// interested in.
    ///
    /// The iterator returns a sequence of points in time given as [`DateTime`] of the recurrences
    /// in this interval. Note that an overlap of the recurrences with this interval is sufficient.
    /// For example, if an recurrence starts before `end`, but ends after `end`, it will still be
    /// delivered by the iterator.
    pub fn dates_between<Tz1: TimeZone, Tz2: TimeZone, Tz3: TimeZone>(
        &self,
        dtstart: DateTime<Tz1>,
        dtdur: Option<Duration>,
        start: DateTime<Tz2>,
        end: DateTime<Tz3>,
    ) -> RecurIterator<'_>
    where
        Tz1::Offset: Copy,
        Tz2::Offset: Copy,
        Tz3::Offset: Copy,
    {
        let dtstart = dtstart.naive_local().and_utc();
        let start = start.naive_local().and_utc();
        let end = end.naive_local().and_utc();
        let interval = self.interval.unwrap_or(1) as u32;
        // go one interval further to ensure that we do not miss an occurrence. for example, if we
        // want to see all occurrences until December 20th of a monthly event starting at 25th of
        // January, we will not consider the December as the 25th is already out of range. going
        // one interval further means that we will consider the December and might set the day to
        // something else, which might indeed be in the range.
        let beyond_end = next_date(end, self.freq, interval).unwrap_or(end);
        let until = if let Some(ref until) = self.until {
            DateContext::local(Tz::UTC)
                .date(until)
                .resolved_end()
                .with_timezone(&Utc)
                .min(beyond_end)
        } else {
            beyond_end
        };

        RecurIterator {
            rrule: self,
            dtstart,
            start,
            end,
            dtdur,
            date: dtstart,
            until,
            count: 0,
            interval,
            last: vec![],
            last_pos: 0,
        }
    }

    fn limited(&self, date: DateTime<Utc>) -> bool {
        if let Some(by_month) = &self.by_month
            && self.freq <= CalRRuleFreq::Monthly
            && !by_month.contains(&(date.month() as u8))
        {
            return true;
        }

        if let Some(by_yday) = &self.by_year_day
            && self.freq <= CalRRuleFreq::Hourly
            && !by_yday.iter().any(|yd| match yd.side {
                CalRRuleSide::Start => yd.num as u32 == date.ordinal(),
                CalRRuleSide::End => {
                    let days = util::year_days(date.year());
                    days - (yd.num - 1) as u32 == date.ordinal()
                }
            })
        {
            return true;
        }
        if let Some(by_mday) = &self.by_mon_day
            && self.freq <= CalRRuleFreq::Daily
            && !by_mday.iter().any(|wd| match wd.side {
                CalRRuleSide::Start => wd.num as u32 == date.day(),
                CalRRuleSide::End => {
                    let days = util::month_days(date.year(), date.month());
                    days - (wd.num - 1) as u32 == date.day()
                }
            })
        {
            return true;
        }

        // num+side is ignored here as this is only applicable for FREQ=MONTHLY|YEARLY
        if let Some(by_day) = &self.by_day
            && self.freq <= CalRRuleFreq::Daily
            && !by_day.iter().any(|wd| wd.day == date.weekday())
        {
            return true;
        }

        // TODO ignore if event has DTSTART=DATE
        if let Some(by_hour) = &self.by_hour
            && self.freq <= CalRRuleFreq::Hourly
            && !by_hour.iter().any(|&h| h as u32 == date.hour())
        {
            return true;
        }
        if let Some(by_min) = &self.by_minute
            && self.freq <= CalRRuleFreq::Minutely
            && !by_min.iter().any(|&m| m as u32 == date.minute())
        {
            return true;
        }
        if let Some(by_sec) = &self.by_second
            && self.freq <= CalRRuleFreq::Secondly
            && !by_sec.iter().any(|&s| s as u32 == date.second())
        {
            return true;
        }

        false
    }

    fn expand(
        &self,
        dtstart: DateTime<Utc>,
        dtdur: Option<Duration>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        date: DateTime<Utc>,
        count: &mut usize,
    ) -> Option<Vec<DateTime<Utc>>> {
        let months = [date.month() as u8];
        let mut months = months.as_slice();
        let mut mon_days = vec![date.day() as u16];
        let hours = [date.hour() as u8];
        let mut hours = hours.as_slice();
        let mins = [date.minute() as u8];
        let mut mins = mins.as_slice();
        let secs = [date.second() as u8];
        let mut secs = secs.as_slice();

        if let Some(by_hour) = &self.by_hour
            && self.freq > CalRRuleFreq::Hourly
        {
            hours = by_hour.as_slice();
        }
        if let Some(by_min) = &self.by_minute
            && self.freq > CalRRuleFreq::Minutely
        {
            mins = by_min.as_slice();
        }
        if let Some(by_sec) = &self.by_second
            && self.freq > CalRRuleFreq::Secondly
        {
            secs = by_sec.as_slice();
        }

        if self.freq > CalRRuleFreq::Monthly
            && (self.by_year_day.is_some() || self.by_week_no.is_some())
        {
            // Build a year-based candidate set (by year-day and/or week-no) and then reuse
            // the standard BYxxx filters so this branch stays aligned with other expansions.
            let mut base_dates = Vec::new();
            if let Some(by_year_day) = &self.by_year_day {
                base_dates.extend(base_dates_from_by_year_day(date, by_year_day));
            }
            if let Some(by_week_no) = &self.by_week_no {
                base_dates.extend(base_dates_from_by_week_no(
                    date,
                    by_week_no,
                    self.week_start,
                ));
            }

            let mut candidates = Vec::new();
            for base in base_dates {
                if !passes_by_month(base, self.by_month.as_ref())
                    || !passes_by_month_day(base, self.by_mon_day.as_ref())
                    || !passes_by_day(base, self.by_day.as_ref(), self)
                {
                    continue;
                }

                for h in hours {
                    for m in mins {
                        for s in secs {
                            if let Some(ndate) = base.with_hour(*h as u32)
                                && let Some(ndate) = ndate.with_minute(*m as u32)
                                && let Some(ndate) = ndate.with_second(*s as u32)
                            {
                                candidates.push(ndate.with_timezone(&Utc));
                            }
                        }
                    }
                }
            }

            candidates.sort();
            candidates.dedup();
            return self.finalize_candidates(candidates, dtstart, dtdur, start, end, count);
        }

        if let Some(by_month) = &self.by_month
            && self.freq > CalRRuleFreq::Monthly
        {
            months = by_month.as_slice();
        }
        if let Some(by_mon_day) = &self.by_mon_day
            && self.freq >= CalRRuleFreq::Monthly
        {
            mon_days = by_mon_day
                .iter()
                .map(|md| match md.side {
                    CalRRuleSide::Start => md.num,
                    CalRRuleSide::End => {
                        util::month_days(date.year(), date.month()) as u16 - (md.num - 1)
                    }
                })
                .collect();
        }
        if self.freq >= CalRRuleFreq::Weekly
            && let Some(by_day) = self.by_day.as_ref()
        {
            // Define a unified period window: [period_start, next_period_start)
            let period_start = match self.freq {
                CalRRuleFreq::Weekly => {
                    let day_of_week = match self.week_start {
                        Some(wkst) => date.weekday().days_since(wkst),
                        None => date.weekday().num_days_from_monday(),
                    };
                    (date.naive_utc() - Duration::days(day_of_week as i64)).and_utc()
                }
                CalRRuleFreq::Monthly => date.with_day(1)?,
                _ => {
                    // Yearly
                    date.with_month(1)?.with_day(1)?
                }
            };

            let period_end = match self.freq {
                CalRRuleFreq::Weekly => {
                    // Advance exactly one week while preserving wall-clock date/time fields.
                    next_date(period_start, CalRRuleFreq::Weekly, 1)?
                }
                CalRRuleFreq::Monthly => {
                    if period_start.month() == 12 {
                        period_start
                            .with_year(period_start.year() + 1)?
                            .with_month(1)?
                    } else {
                        period_start.with_month(period_start.month() + 1)?
                    }
                }
                _ => {
                    // Yearly
                    period_start.with_year(period_start.year() + 1)?
                }
            };

            let mut vcur = period_start;
            let vend = period_end;

            let mut candidates = vec![];
            while vcur < vend {
                // limit by month if BYMONTH is present
                if self.by_month.is_none() || months.contains(&(vcur.month() as u8)) {
                    for h in hours {
                        for m in mins {
                            for s in secs {
                                if by_day.iter().any(|d| d.matches(vcur, self))
                                    && let Some(ndate) = vcur.with_hour(*h as u32)
                                    && let Some(ndate) = ndate.with_minute(*m as u32)
                                    && let Some(ndate) = ndate.with_second(*s as u32)
                                {
                                    candidates.push(ndate.with_timezone(&Utc));
                                }
                            }
                        }
                    }
                }
                vcur = next_date(vcur, CalRRuleFreq::Daily, 1).unwrap();
            }
            return self.finalize_candidates(candidates, dtstart, dtdur, start, end, count);
        }

        let mut candidates = vec![];
        for mon in months {
            for d in &mon_days {
                for h in hours {
                    for m in mins {
                        for s in secs {
                            if let Some(ndate) = date.with_month(*mon as u32)
                                && let Some(ndate) = ndate.with_day(*d as u32)
                                && let Some(ndate) = ndate.with_hour(*h as u32)
                                && let Some(ndate) = ndate.with_minute(*m as u32)
                                && let Some(ndate) = ndate.with_second(*s as u32)
                            {
                                candidates.push(ndate.with_timezone(&Utc));
                            }
                        }
                    }
                }
            }
        }
        self.finalize_candidates(candidates, dtstart, dtdur, start, end, count)
    }

    fn finalize_candidates(
        &self,
        candidates: Vec<DateTime<Utc>>,
        dtstart: DateTime<Utc>,
        dtdur: Option<Duration>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        count: &mut usize,
    ) -> Option<Vec<DateTime<Utc>>> {
        let selected = self.apply_by_set_pos(candidates);
        let mut res = Vec::new();
        for ndate in selected {
            if ndate < dtstart {
                continue;
            }
            // Compute the effective end of this candidate occurrence in wall-clock time so
            // that a DST transition between the candidate start and its end does not shift
            // the end by the DST delta.
            let nend = dtdur.map_or(ndate, |d| ndate + d);
            if util::date_ranges_overlap(ndate, nend, start, end) {
                res.push(ndate);
            }
            *count += 1;

            if let Some(rcount) = self.count
                && *count >= rcount as usize
            {
                if !res.is_empty() {
                    return Some(res);
                }
                return None;
            }
        }
        Some(res)
    }

    fn apply_by_set_pos(&self, mut res: Vec<DateTime<Utc>>) -> Vec<DateTime<Utc>> {
        let Some(by_set_pos) = self.by_set_pos.as_ref() else {
            return res;
        };

        if res.is_empty() {
            return res;
        }

        res.sort();

        let len = res.len() as i32;
        let mut picked = Vec::new();
        for pos in by_set_pos {
            let index = match pos.side {
                CalRRuleSide::Start => pos.num as i32 - 1,
                CalRRuleSide::End => len - pos.num as i32,
            };
            if index >= 0 && index < len {
                picked.push(res[index as usize]);
            }
        }

        picked.sort();
        picked.dedup();
        picked
    }

    fn has_any_by(&self) -> bool {
        self.by_second.is_some()
            || self.by_minute.is_some()
            || self.by_hour.is_some()
            || self.by_day.is_some()
            || self.by_mon_day.is_some()
            || self.by_year_day.is_some()
            || self.by_week_no.is_some()
            || self.by_month.is_some()
    }

    /// Returns a human-readable representation of the repeat interval of this recurrence rule,
    /// without the termination information (UNTIL/COUNT).
    ///
    /// For example, it could say "Weekly, on Thursday".
    pub fn human_interval<'l>(&self, locale: &'l dyn CalLocale) -> RRuleHumanInterval<'_, 'l> {
        RRuleHumanInterval {
            rrule: self,
            locale,
        }
    }

    /// Returns a human-readable representation of this recurrence rule, including termination
    /// information (UNTIL/COUNT) on a second line when present.
    ///
    /// For example, it could say "Every 2 years\nRepeats 5 times".
    pub fn human<'l>(&self, locale: &'l dyn CalLocale) -> RRuleHuman<'_, 'l> {
        RRuleHuman {
            rrule: self,
            locale,
        }
    }
}

/// Implements [`Display`](fmt::Display) to render the repeat interval phrase of a [`CalRRule`],
/// without any termination information (UNTIL/COUNT).
///
/// For example, it could say "Weekly, on Thursday" or "Every 2 years".
pub struct RRuleHumanInterval<'r, 'l> {
    rrule: &'r CalRRule,
    locale: &'l dyn CalLocale,
}

impl fmt::Display for RRuleHumanInterval<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.rrule.interval {
            Some(interval) if interval > 1 => {
                write!(f, "{} ", self.locale.translate("Every"))?;
                match self.rrule.freq {
                    CalRRuleFreq::Yearly => write!(
                        f,
                        "{}",
                        formatx!(self.locale.translate("{} years"), interval).unwrap()
                    )?,
                    CalRRuleFreq::Monthly => write!(
                        f,
                        "{}",
                        formatx!(self.locale.translate("{} months"), interval).unwrap()
                    )?,
                    CalRRuleFreq::Weekly => write!(
                        f,
                        "{}",
                        formatx!(self.locale.translate("{} weeks"), interval).unwrap()
                    )?,
                    CalRRuleFreq::Daily => write!(
                        f,
                        "{}",
                        formatx!(self.locale.translate("{} days"), interval).unwrap()
                    )?,
                    CalRRuleFreq::Hourly => write!(
                        f,
                        "{}",
                        formatx!(self.locale.translate("{} hours"), interval).unwrap()
                    )?,
                    CalRRuleFreq::Minutely => write!(
                        f,
                        "{}",
                        formatx!(self.locale.translate("{} minutes"), interval).unwrap()
                    )?,
                    CalRRuleFreq::Secondly => write!(
                        f,
                        "{}",
                        formatx!(self.locale.translate("{} seconds"), interval).unwrap()
                    )?,
                }
            }
            _ => {
                let freq = format!("{:?}", self.rrule.freq);
                write!(f, "{}", self.locale.translate(&freq))?;
            }
        }

        if let Some(by_month) = &self.rrule.by_month {
            let months = by_month
                .iter()
                .map(|no| {
                    self.locale
                        .translate(&format!("{:?}", Month::try_from(*no).unwrap()))
                        .to_string()
                })
                .collect::<Vec<_>>();
            write!(
                f,
                ", {}",
                formatx!(
                    self.locale.translate("in {}"),
                    util::human_list(&months, self.locale)
                )
                .unwrap()
            )?;
        }

        if let Some(by_day) = &self.rrule.by_day {
            let days = by_day
                .iter()
                .map(|d| format!("{}", d.human(self.locale)))
                .collect::<Vec<_>>();
            write!(
                f,
                ", {}",
                formatx!(
                    self.locale.translate("on {}"),
                    util::human_list(&days, self.locale)
                )
                .unwrap()
            )?;
        }

        if let Some(by_mon_day) = &self.rrule.by_mon_day {
            let days = by_mon_day
                .iter()
                .map(|d| format!("{}", d.human(self.locale)))
                .collect::<Vec<_>>();
            write!(
                f,
                ", {}",
                formatx!(
                    self.locale.translate("on the {} day of the month"),
                    util::human_list(&days, self.locale)
                )
                .unwrap()
            )?;
        }

        if let Some(by_year_day) = &self.rrule.by_year_day {
            let days = by_year_day
                .iter()
                .map(|d| format!("{}", d.human(self.locale)))
                .collect::<Vec<_>>();
            write!(
                f,
                ", {}",
                formatx!(
                    self.locale.translate("on the {} day of the year"),
                    util::human_list(&days, self.locale)
                )
                .unwrap()
            )?;
        }

        if let Some(by_hour) = &self.rrule.by_hour {
            let hours = by_hour.iter().map(|d| format!("{d}")).collect::<Vec<_>>();
            write!(
                f,
                ", {}",
                formatx!(
                    self.locale.translate("at hour(s) {}"),
                    util::human_list(&hours, self.locale)
                )
                .unwrap()
            )?;
        }

        if let Some(by_minute) = &self.rrule.by_minute {
            let mins = by_minute.iter().map(|d| format!("{d}")).collect::<Vec<_>>();
            write!(
                f,
                ", {}",
                formatx!(
                    self.locale.translate("at minute(s) {}"),
                    util::human_list(&mins, self.locale)
                )
                .unwrap()
            )?;
        }

        if let Some(by_second) = &self.rrule.by_second {
            let secs = by_second.iter().map(|d| format!("{d}")).collect::<Vec<_>>();
            write!(
                f,
                ", {}",
                formatx!(
                    self.locale.translate("at second(s) {}"),
                    util::human_list(&secs, self.locale)
                )
                .unwrap()
            )?;
        }

        Ok(())
    }
}

/// Implements [`Display`](fmt::Display) to create a human-readable representation of a
/// [`CalRRule`], including termination information (UNTIL/COUNT) on a second line when present.
///
/// For example, it could say "Every 2 years\nRepeats 5 times".
pub struct RRuleHuman<'r, 'l> {
    rrule: &'r CalRRule,
    locale: &'l dyn CalLocale,
}

impl fmt::Display for RRuleHuman<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.rrule.human_interval(self.locale))?;

        if let Some(until) = &self.rrule.until {
            write!(
                f,
                "\n{}",
                formatx!(
                    self.locale.translate("Repeats until {}"),
                    self.locale.fmt_naive_date(&until.as_naive_date())
                )
                .unwrap()
            )?;
        } else if let Some(count) = self.rrule.count {
            write!(
                f,
                "\n{}",
                formatx!(self.locale.translate("Repeats {} times"), count).unwrap()
            )?;
        }
        Ok(())
    }
}

fn write_list<I, T>(l: Option<I>, name: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result
where
    I: Iterator<Item = T>,
    T: fmt::Display,
{
    if let Some(l) = l {
        write!(f, ";{}={}", name, l.map(|v| format!("{v}")).join(","))
    } else {
        Ok(())
    }
}

impl fmt::Display for CalRRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FREQ={}", self.freq)?;
        if let Some(count) = self.count {
            write!(f, ";COUNT={count}")?;
        }
        if let Some(interval) = self.interval {
            write!(f, ";INTERVAL={interval}")?;
        }

        write_list(self.by_second.as_ref().map(|v| v.iter()), "BYSECOND", f)?;
        write_list(self.by_minute.as_ref().map(|v| v.iter()), "BYMINUTE", f)?;
        write_list(self.by_hour.as_ref().map(|v| v.iter()), "BYHOUR", f)?;
        write_list(self.by_day.as_ref().map(|v| v.iter()), "BYDAY", f)?;
        write_list(self.by_mon_day.as_ref().map(|v| v.iter()), "BYMONTHDAY", f)?;
        write_list(self.by_year_day.as_ref().map(|v| v.iter()), "BYYEARDAY", f)?;
        write_list(self.by_week_no.as_ref().map(|v| v.iter()), "BYWEEKNO", f)?;
        write_list(self.by_month.as_ref().map(|v| v.iter()), "BYMONTH", f)?;
        write_list(self.by_set_pos.as_ref().map(|v| v.iter()), "BYSETPOS", f)?;
        if let Some(week_start) = self.week_start {
            write!(f, ";WKST={}", CalWDayDesc::to_weekday_str(week_start))?;
        }
        if let Some(ref until) = self.until {
            let prop = until.to_prop("");
            write!(f, ";UNTIL={}", prop.value())?;
        }
        Ok(())
    }
}

fn parse_list<E, T>(s: &str) -> Result<Vec<T>, ParseError>
where
    T: FromStr<Err = E>,
    ParseError: From<E>,
{
    let mut list = Vec::new();
    for item in s.split(',') {
        list.push(item.parse()?);
    }
    Ok(list)
}

impl FromStr for CalRRule {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut rrule = CalRRule::default();

        let mut seen_freq = false;
        for part in s.split(';') {
            let mut name_value = part.splitn(2, '=');
            let name = name_value.next().unwrap();
            let value = name_value.next().ok_or(ParseError::MissingParamValue)?;
            match name {
                "FREQ" => {
                    rrule.freq = value.parse()?;
                    seen_freq = true;
                }
                "UNTIL" => {
                    let prop: Property = format!("UNTIL:{value}").parse()?;
                    rrule.until = Some(prop.try_into()?);
                }
                "COUNT" => {
                    rrule.count = Some(value.parse()?);
                }
                "INTERVAL" => {
                    rrule.interval = Some(value.parse()?);
                }
                "BYSECOND" => {
                    rrule.by_second = Some(parse_list(value)?);
                }
                "BYMINUTE" => {
                    rrule.by_minute = Some(parse_list(value)?);
                }
                "BYHOUR" => {
                    rrule.by_hour = Some(parse_list(value)?);
                }
                "BYDAY" => {
                    rrule.by_day = Some(parse_list(value)?);
                }
                "BYMONTHDAY" => {
                    rrule.by_mon_day = Some(parse_list(value)?);
                }
                "BYYEARDAY" => {
                    rrule.by_year_day = Some(parse_list(value)?);
                }
                "BYWEEKNO" => {
                    rrule.by_week_no = Some(parse_list(value)?);
                }
                "BYMONTH" => {
                    rrule.by_month = Some(parse_list(value)?);
                }
                "BYSETPOS" => {
                    rrule.by_set_pos = Some(parse_list(value)?);
                }
                "WKST" => {
                    rrule.week_start = Some(CalWDayDesc::parse_weekday(value)?);
                }
                _ => return Err(ParseError::UnexpectedRRule(name.to_string())),
            }
        }

        if !seen_freq {
            return Err(ParseError::UnexpectedRRule("Missing FREQ".to_string()));
        }

        if rrule.count.is_some() && rrule.until.is_some() {
            return Err(ParseError::UnexpectedRRule(
                "COUNT and UNTIL must not both be present".to_string(),
            ));
        }

        if let Some(by_set_pos) = &rrule.by_set_pos {
            if !rrule.has_any_by() {
                return Err(ParseError::UnexpectedRRule(
                    "BYSETPOS must be used with another BYxxx rule".to_string(),
                ));
            }
            for pos in by_set_pos {
                if pos.num == 0 || pos.num > 366 {
                    return Err(ParseError::UnexpectedRRule(
                        "BYSETPOS must be in range 1..366 or -1..-366".to_string(),
                    ));
                }
            }
        }

        Ok(rrule)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc, Weekday};

    use crate::objects::CalLocaleEn;
    use crate::objects::date::CalDateTime;

    use super::*;

    // --- CalRRuleFreq ---

    #[test]
    fn freq_display_all_variants() {
        assert_eq!(format!("{}", CalRRuleFreq::Secondly), "SECONDLY");
        assert_eq!(format!("{}", CalRRuleFreq::Minutely), "MINUTELY");
        assert_eq!(format!("{}", CalRRuleFreq::Hourly), "HOURLY");
        assert_eq!(format!("{}", CalRRuleFreq::Daily), "DAILY");
        assert_eq!(format!("{}", CalRRuleFreq::Weekly), "WEEKLY");
        assert_eq!(format!("{}", CalRRuleFreq::Monthly), "MONTHLY");
        assert_eq!(format!("{}", CalRRuleFreq::Yearly), "YEARLY");
    }

    #[test]
    fn freq_parse_invalid() {
        assert!("FORTNIGHTLY".parse::<CalRRuleFreq>().is_err());
        assert!("".parse::<CalRRuleFreq>().is_err());
    }

    // --- CalRRuleSide ---

    #[test]
    fn side_parse_invalid() {
        // Any character other than '+' or '-' at position 0 must yield an error.
        assert!("X1MO".parse::<CalRRuleSide>().is_err());
    }

    // --- CalWDayDesc ---

    #[test]
    fn wday_getters_day_and_nth() {
        let desc = CalWDayDesc::new(Weekday::Wed, Some((3, CalRRuleSide::End)));
        assert_eq!(desc.day(), Weekday::Wed);
        assert_eq!(desc.nth(), Some((3, CalRRuleSide::End)));

        let no_nth = CalWDayDesc::new(Weekday::Fri, None);
        assert_eq!(no_nth.day(), Weekday::Fri);
        assert_eq!(no_nth.nth(), None);
    }

    #[test]
    fn to_weekday_str_all_variants() {
        assert_eq!(CalWDayDesc::to_weekday_str(Weekday::Mon), "MO");
        assert_eq!(CalWDayDesc::to_weekday_str(Weekday::Tue), "TU");
        assert_eq!(CalWDayDesc::to_weekday_str(Weekday::Wed), "WE");
        assert_eq!(CalWDayDesc::to_weekday_str(Weekday::Thu), "TH");
        assert_eq!(CalWDayDesc::to_weekday_str(Weekday::Fri), "FR");
        assert_eq!(CalWDayDesc::to_weekday_str(Weekday::Sat), "SA");
        assert_eq!(CalWDayDesc::to_weekday_str(Weekday::Sun), "SU");
    }

    #[test]
    fn wday_display() {
        // Plain weekday — no nth component.
        let plain = CalWDayDesc::new(Weekday::Thu, None);
        assert_eq!(format!("{plain}"), "TH");

        // Positive nth prefix.
        let start = CalWDayDesc::new(Weekday::Mon, Some((2, CalRRuleSide::Start)));
        assert_eq!(format!("{start}"), "+2MO");

        // Negative nth prefix.
        let end = CalWDayDesc::new(Weekday::Fri, Some((1, CalRRuleSide::End)));
        assert_eq!(format!("{end}"), "-1FR");
    }

    #[test]
    fn wday_parse_empty_after_sign_is_error() {
        // A bare sign with no weekday letters must be an error.
        assert!("-".parse::<CalWDayDesc>().is_err());
    }

    #[test]
    fn wday_parse_invalid_weekday_abbrev() {
        assert!("XX".parse::<CalWDayDesc>().is_err());
        assert!("MX".parse::<CalWDayDesc>().is_err());
    }

    #[test]
    fn wday_matches_invalid_freq_returns_false() {
        // When a numbered weekday descriptor (nth is Some) is used with a frequency other than
        // Weekly, Monthly, or Yearly, matches() must return false.
        let desc = CalWDayDesc::new(Weekday::Mon, Some((1, CalRRuleSide::Start)));
        let rrule = CalRRule {
            freq: CalRRuleFreq::Daily,
            by_day: Some(vec![desc]),
            ..Default::default()
        };
        let date = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 9, 2, 9, 0, 0)
            .unwrap()
            .naive_local()
            .and_utc();
        assert!(!desc.matches(date, &rrule));
    }

    // --- CalRRule getters and setters ---

    #[test]
    fn rrule_getters_and_setters() {
        let mut rule = CalRRule::default();

        // frequency
        assert_eq!(rule.frequency(), CalRRuleFreq::Weekly);
        rule.set_frequency(CalRRuleFreq::Daily);
        assert_eq!(rule.frequency(), CalRRuleFreq::Daily);

        // count
        assert_eq!(rule.count(), None);
        rule.set_count(5);
        assert_eq!(rule.count(), Some(5));

        // interval
        assert_eq!(rule.interval(), None);
        rule.set_interval(3);
        assert_eq!(rule.interval(), Some(3));

        // until (must clear count first so the rule stays valid)
        rule.set_count(0); // reset count to avoid count+until conflict at parse time (not relevant here)
        assert_eq!(rule.until(), None);
        let until_date = CalDate::DateTime(CalDateTime::Utc(
            Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap(),
        ));
        rule.set_until(until_date.clone());
        assert_eq!(rule.until(), Some(&until_date));

        // by_day
        assert_eq!(rule.by_day(), None);
        let days = vec![CalWDayDesc::new(Weekday::Mon, None)];
        rule.set_by_day(Some(days.clone()));
        assert_eq!(rule.by_day(), Some(&days));
        rule.set_by_day(None);
        assert_eq!(rule.by_day(), None);
    }

    // --- CalRRule::FromStr error paths ---

    #[test]
    fn rrule_parse_unknown_key_is_error() {
        assert!("FREQ=DAILY;UNKNOWN=VALUE".parse::<CalRRule>().is_err());
    }

    // --- CalRRule::Display ---

    #[test]
    fn rrule_display_round_trips_all_fields() {
        // Build a rule with every serialisable field set and verify that the Display output
        // round-trips through FromStr to produce an equal rule.
        let rule: CalRRule = "FREQ=WEEKLY;COUNT=5;INTERVAL=2;BYDAY=MO,TU;WKST=SU"
            .parse()
            .unwrap();
        let displayed = format!("{rule}");
        let reparsed: CalRRule = displayed.parse().unwrap();
        assert_eq!(rule, reparsed);
    }

    #[test]
    fn rrule_display_with_until() {
        let rule: CalRRule = "FREQ=DAILY;UNTIL=20250101T000000Z".parse().unwrap();
        let displayed = format!("{rule}");
        // Verify the exact UNTIL timestamp is present in the serialised output.
        assert!(
            displayed.contains("UNTIL=20250101T000000Z"),
            "expected exact UNTIL timestamp in: {displayed}"
        );
        let reparsed: CalRRule = displayed.parse().unwrap();
        assert_eq!(rule, reparsed);
    }

    #[test]
    fn rrule_display_with_byx() {
        // All BYxxx fields use DayDesc::Display, which emits plain integers compatible with
        // FromStr, so every field in this rule must survive a full Display → parse round-trip.
        // The Display ordering is:
        // FREQ, BYSECOND, BYMINUTE, BYHOUR, BYDAY, BYMONTHDAY, BYYEARDAY, BYWEEKNO, BYMONTH,
        // BYSETPOS
        let rule: CalRRule = "FREQ=YEARLY;BYSECOND=10;BYMINUTE=15;BYHOUR=8;BYMONTHDAY=5;BYMONTH=3;\
             BYYEARDAY=100;BYWEEKNO=10;BYSETPOS=1;BYDAY=MO"
            .parse()
            .unwrap();
        let displayed = format!("{rule}");

        let expected = "FREQ=YEARLY;BYSECOND=10;BYMINUTE=15;BYHOUR=8;BYDAY=MO;\
                        BYMONTHDAY=5;BYYEARDAY=100;BYWEEKNO=10;BYMONTH=3;BYSETPOS=1";
        assert_eq!(
            displayed, expected,
            "expected exact serialisation for BYx fields"
        );

        let reparsed: CalRRule = displayed.parse().unwrap();
        assert_eq!(rule, reparsed);
    }

    #[test]
    fn rrule_display_with_wkst() {
        let rule: CalRRule = "FREQ=WEEKLY;BYDAY=TU,TH;WKST=MO".parse().unwrap();
        let displayed = format!("{rule}");
        assert!(displayed.contains("WKST=MO"));
        let reparsed: CalRRule = displayed.parse().unwrap();
        assert_eq!(rule, reparsed);
    }

    // --- RRuleHuman::Display ---

    #[test]
    fn human_every_n_units() {
        let rule: CalRRule = "FREQ=YEARLY;INTERVAL=3".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Every 3 years");

        let rule: CalRRule = "FREQ=MONTHLY;INTERVAL=6".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Every 6 months");

        let rule: CalRRule = "FREQ=HOURLY;INTERVAL=4".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Every 4 hours");

        let rule: CalRRule = "FREQ=MINUTELY;INTERVAL=30".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Every 30 minutes");

        let rule: CalRRule = "FREQ=SECONDLY;INTERVAL=15".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Every 15 seconds");
    }

    #[test]
    fn human_interval_1_falls_back_to_freq_name() {
        // INTERVAL=1 (or absent) should display just the frequency name.
        let rule: CalRRule = "FREQ=SECONDLY".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Secondly");

        let rule: CalRRule = "FREQ=MINUTELY".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Minutely");

        let rule: CalRRule = "FREQ=HOURLY".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Hourly");

        let rule: CalRRule = "FREQ=WEEKLY".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Weekly");

        let rule: CalRRule = "FREQ=MONTHLY".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Monthly");

        let rule: CalRRule = "FREQ=YEARLY".parse().unwrap();
        assert_eq!(format!("{}", rule.human(&CalLocaleEn)), "Yearly");
    }

    #[test]
    fn human_with_by_year_day() {
        let rule: CalRRule = "FREQ=YEARLY;BYYEARDAY=1,-1".parse().unwrap();
        let text = format!("{}", rule.human(&CalLocaleEn));
        let expected = "Yearly, on the 1st and last day of the year";
        assert_eq!(text, expected, "got: {text}");
    }

    #[test]
    fn human_with_by_mon_day() {
        let rule: CalRRule = "FREQ=MONTHLY;BYMONTHDAY=1,-1".parse().unwrap();
        let text = format!("{}", rule.human(&CalLocaleEn));
        let expected = "Monthly, on the 1st and last day of the month";
        assert_eq!(text, expected, "got: {text}");
    }

    #[test]
    fn human_with_by_hour_minute_second() {
        let rule: CalRRule = "FREQ=DAILY;BYHOUR=9,17;BYMINUTE=0,30;BYSECOND=0"
            .parse()
            .unwrap();
        let text = format!("{}", rule.human(&CalLocaleEn));
        let expected = "Daily, at hour(s) 9 and 17, at minute(s) 0 and 30, at second(s) 0";
        assert_eq!(text, expected, "got: {text}");
    }

    #[test]
    fn human_repeats_until_and_in_month() {
        let rule: CalRRule = "FREQ=YEARLY;BYMONTH=6;UNTIL=20301231T000000Z"
            .parse()
            .unwrap();
        let text = format!("{}", rule.human(&CalLocaleEn));
        let expected = "Yearly, in June\nRepeats until December 31, 2030";
        assert_eq!(text, expected, "got: {text}");
    }

    // --- limited() BYMONTH with sub-monthly frequencies ---

    #[test]
    fn limited_by_month_filters_secondly_freq() {
        // BYMONTH=3 with FREQ=SECONDLY means occurrences in months other than March are
        // filtered out by limited(). Verify via dates_between producing nothing outside March.
        let dtstart = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 3, 1, 0, 0, 0)
            .unwrap();
        let rrule: CalRRule = "FREQ=SECONDLY;COUNT=3;BYMONTH=3;BYHOUR=0;BYMINUTE=0;BYSECOND=0,1,2"
            .parse()
            .unwrap();
        let mut iter = rrule.dates_between(dtstart, None, dtstart, dtstart + Duration::days(60));
        // All three occurrences must fall within March.
        for _ in 0..3 {
            let d = iter.next().unwrap();
            assert_eq!(d.month(), 3, "expected March, got month {}", d.month());
        }
        assert_eq!(iter.next(), None);
    }

    // --- expand() BYMINUTE expansion with FREQ=MINUTELY ---

    #[test]
    fn range_minutely_byminute_expand() {
        // FREQ=MINUTELY with BYMINUTE expands per-minute occurrences at the listed minute values.
        let dtstart = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 9, 2, 9, 0, 0)
            .unwrap();
        let rrule: CalRRule = "FREQ=MINUTELY;COUNT=4;BYMINUTE=0,15,30,45".parse().unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::minutes(1)),
            dtstart,
            dtstart + Duration::hours(2),
        );
        // The rule starts at 09:00 and should hit :00, :15, :30, :45 of 09:xx then stop.
        let expected_minutes = [0u32, 15, 30, 45];
        for exp_min in expected_minutes {
            let d = iter.next().unwrap();
            assert_eq!(
                d.minute(),
                exp_min,
                "expected :{exp_min:02}, got :{}",
                d.minute()
            );
        }
        assert_eq!(iter.next(), None);
    }

    // --- BYWEEKNO with end-side (negative week number) ---

    #[test]
    fn range_byweekno_end_side() {
        // -1 in BYWEEKNO means the last week of the year. With BYDAY=MO we get the last Monday
        // of the last week of each year.
        let dtstart = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 1, 9, 0, 0)
            .unwrap();
        let rrule: CalRRule = "FREQ=YEARLY;COUNT=2;BYWEEKNO=-1;BYDAY=MO".parse().unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            dtstart + Duration::days(800),
        );
        let first = iter.next().unwrap();
        let second = iter.next().unwrap();
        assert_eq!(iter.next(), None);
        // Both occurrences must be Mondays in December (last week of year).
        assert_eq!(first.weekday(), Weekday::Mon);
        assert_eq!(second.weekday(), Weekday::Mon);
        assert!(first.month() == 12 || first.month() == 1);
    }

    // --- BYYEARDAY with end-side (negative year-day) ---

    #[test]
    fn range_byyearday_end_side() {
        // -1 means the last day of the year (December 31).
        let dtstart = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 1, 9, 0, 0)
            .unwrap();
        let rrule: CalRRule = "FREQ=YEARLY;COUNT=2;BYYEARDAY=-1".parse().unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            dtstart + Duration::days(800),
        );
        let first = iter.next().unwrap();
        let second = iter.next().unwrap();
        assert_eq!(iter.next(), None);
        assert_eq!(first.month(), 12);
        assert_eq!(first.day(), 31);
        assert_eq!(second.month(), 12);
        assert_eq!(second.day(), 31);
        assert_eq!(second.year() - first.year(), 1);
    }

    // --- passes_by_month_day via BYWEEKNO + BYMONTHDAY ---

    #[test]
    fn range_byweekno_with_bymonthday_start_side() {
        // BYWEEKNO=1 limits to the first ISO week; BYMONTHDAY=1 further restricts to the 1st of
        // the month.  The only day satisfying both is January 1 when it falls in week 1.
        let dtstart = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 1, 9, 0, 0)
            .unwrap();
        let rrule: CalRRule = "FREQ=YEARLY;COUNT=1;BYWEEKNO=1;BYMONTHDAY=1"
            .parse()
            .unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            dtstart + Duration::days(400),
        );
        let d = iter.next().unwrap();
        assert_eq!(d.month(), 1);
        assert_eq!(d.day(), 1);
    }

    #[test]
    fn range_byweekno_with_bymonthday_end_side() {
        // -1 in BYMONTHDAY means the last day of the month combined with BYWEEKNO=20.
        // We use COUNT=1 to stop quickly and just verify no panic / end-side arithmetic runs.
        let dtstart = chrono_tz::America::New_York
            .with_ymd_and_hms(1997, 1, 1, 9, 0, 0)
            .unwrap();
        let rrule: CalRRule = "FREQ=YEARLY;COUNT=1;BYWEEKNO=20;BYMONTHDAY=-1"
            .parse()
            .unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            dtstart + Duration::days(400),
        );
        // The combined filter may or may not yield a result; what matters is no panic.
        let _ = iter.next();
    }

    // --- passes_by_month Some branch via BYYEARDAY + BYMONTH ---

    #[test]
    fn range_byyearday_with_bymonth_filter() {
        // BYYEARDAY=1 (Jan 1) with BYMONTH=1 should yield Jan 1 each year.
        // BYMONTH=2 would filter it out entirely.
        let dtstart = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 1, 9, 0, 0)
            .unwrap();
        let rrule_pass: CalRRule = "FREQ=YEARLY;COUNT=2;BYYEARDAY=1;BYMONTH=1".parse().unwrap();
        let mut iter = rrule_pass.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            dtstart + Duration::days(800),
        );
        assert_eq!(iter.next().unwrap().month(), 1);
        assert_eq!(iter.next().unwrap().month(), 1);
        assert_eq!(iter.next(), None);

        // BYMONTH=2 should give nothing since BYYEARDAY=1 is always January.
        let rrule_fail: CalRRule = "FREQ=YEARLY;COUNT=2;BYYEARDAY=1;BYMONTH=2".parse().unwrap();
        let mut iter2 = rrule_fail.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            dtstart + Duration::days(800),
        );
        assert_eq!(iter2.next(), None);
    }

    // --- finalize_candidates: COUNT limit reached inside a multi-date batch ---

    #[test]
    fn count_cutoff_in_multi_date_batch() {
        // FREQ=MONTHLY with BYMONTHDAY=1,15 produces two candidates per month.  Setting COUNT=3
        // means we get both dates from month 1, then only the first from month 2 — the cutoff
        // happens partway through the second batch, exercising the early-return path.
        let dtstart = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 1, 9, 0, 0)
            .unwrap();
        let rrule: CalRRule = "FREQ=MONTHLY;COUNT=3;BYMONTHDAY=1,15".parse().unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            dtstart + Duration::days(120),
        );
        assert_eq!(iter.next().unwrap().day(), 1); // Jan 1
        assert_eq!(iter.next().unwrap().day(), 15); // Jan 15
        assert_eq!(iter.next().unwrap().day(), 1); // Feb 1  — COUNT=3 reached here
        assert_eq!(iter.next(), None);
    }

    // --- apply_by_set_pos: empty candidate set is a no-op ---

    #[test]
    fn bysetpos_with_empty_candidate_set_yields_nothing() {
        // BYMONTHDAY=31 with BYSETPOS=1 in a month that has fewer than 31 days (February) means
        // the candidate list is empty; apply_by_set_pos must return an empty vec rather than
        // panicking or selecting a wrong element.
        let dtstart = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 2, 1, 9, 0, 0)
            .unwrap();
        // BYMONTH=2 restricts the rule to February only. February never has a 31st day, so
        // apply_by_set_pos receives an empty candidate list and must return nothing rather than
        // panicking or wrapping around.
        let rrule: CalRRule = "FREQ=MONTHLY;COUNT=1;BYMONTH=2;BYMONTHDAY=31;BYSETPOS=1"
            .parse()
            .unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            dtstart + Duration::days(60),
        );
        // February has no 31st, so nothing should be returned within the window.
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn parse_weekday_desc() {
        assert_eq!(
            "MO".parse::<CalWDayDesc>().unwrap(),
            CalWDayDesc::new(Weekday::Mon, None)
        );
        assert_eq!(
            "-3SA".parse::<CalWDayDesc>().unwrap(),
            CalWDayDesc::new(Weekday::Sat, Some((3, CalRRuleSide::End)))
        );
        assert_eq!(
            "+1TU".parse::<CalWDayDesc>().unwrap(),
            CalWDayDesc::new(Weekday::Tue, Some((1, CalRRuleSide::Start)))
        );
        assert_eq!(
            "1FR".parse::<CalWDayDesc>().unwrap(),
            CalWDayDesc::new(Weekday::Fri, Some((1, CalRRuleSide::Start)))
        );
    }

    #[test]
    fn parse_day_desc() {
        assert_eq!(
            "4".parse::<DayDesc>().unwrap(),
            DayDesc::new(4, CalRRuleSide::Start)
        );
        assert_eq!(
            "17".parse::<DayDesc>().unwrap(),
            DayDesc::new(17, CalRRuleSide::Start)
        );
        assert_eq!(
            "-20".parse::<DayDesc>().unwrap(),
            DayDesc::new(20, CalRRuleSide::End)
        );
        assert_eq!(
            "+19".parse::<DayDesc>().unwrap(),
            DayDesc::new(19, CalRRuleSide::Start)
        );
    }

    #[test]
    fn parse_recur_count() {
        let rule = CalRRule {
            freq: CalRRuleFreq::Daily,
            count: Some(10),
            ..Default::default()
        };
        assert_eq!("FREQ=DAILY;COUNT=10".parse::<CalRRule>().unwrap(), rule);
        assert_eq!(
            format!("{}", rule.human(&CalLocaleEn)),
            "Daily\nRepeats 10 times".to_string()
        );
    }

    #[test]
    fn parse_recur_interval() {
        let rule = CalRRule {
            freq: CalRRuleFreq::Monthly,
            interval: Some(2),
            ..Default::default()
        };
        assert_eq!("FREQ=MONTHLY;INTERVAL=2".parse::<CalRRule>().unwrap(), rule);
        assert_eq!(
            format!("{}", rule.human(&CalLocaleEn)),
            "Every 2 months".to_string()
        );
    }

    #[test]
    fn rrule_without_freq_is_rejected() {
        // RFC 5545 §3.3.10: FREQ is required
        let result = "COUNT=10".parse::<CalRRule>();
        assert!(result.is_err(), "RRULE without FREQ must be rejected");
    }

    #[test]
    fn rrule_with_count_and_until_is_rejected() {
        // RFC 5545 §3.3.10: COUNT and UNTIL MUST NOT both occur
        let result = "FREQ=DAILY;COUNT=10;UNTIL=20250101T000000Z".parse::<CalRRule>();
        assert!(
            result.is_err(),
            "RRULE with both COUNT and UNTIL must be rejected"
        );
    }

    #[test]
    fn rrule_count_above_255_is_supported() {
        // RFC allows arbitrary positive integers for COUNT
        let result = "FREQ=DAILY;COUNT=300".parse::<CalRRule>();
        assert!(result.is_ok(), "RRULE with COUNT > 255 should be supported");
        let rule = result.unwrap();
        assert_eq!(rule.count, Some(300));
    }

    #[test]
    fn parse_recur_until() {
        let rule = CalRRule {
            freq: CalRRuleFreq::Daily,
            until: Some(CalDate::DateTime(CalDateTime::Utc(
                Utc.with_ymd_and_hms(1997, 12, 24, 0, 0, 0).unwrap(),
            ))),
            ..Default::default()
        };
        assert_eq!(
            "FREQ=DAILY;UNTIL=19971224T000000Z"
                .parse::<CalRRule>()
                .unwrap(),
            rule
        );
    }

    #[test]
    fn parse_recur_by() {
        let rule = CalRRule {
            freq: CalRRuleFreq::Yearly,
            by_month: Some(vec![1]),
            by_set_pos: Some(vec![
                DayDesc::new(2, CalRRuleSide::Start),
                DayDesc::new(5, CalRRuleSide::Start),
            ]),
            by_day: Some(vec![
                CalWDayDesc::new(Weekday::Sun, None),
                CalWDayDesc::new(Weekday::Mon, None),
                CalWDayDesc::new(Weekday::Tue, None),
                CalWDayDesc::new(Weekday::Wed, None),
                CalWDayDesc::new(Weekday::Thu, None),
                CalWDayDesc::new(Weekday::Fri, None),
                CalWDayDesc::new(Weekday::Sat, None),
            ]),
            ..Default::default()
        };

        assert_eq!(
            "FREQ=YEARLY;BYMONTH=1;BYDAY=SU,MO,TU,WE,TH,FR,SA;BYSETPOS=2,+5"
                .parse::<CalRRule>()
                .unwrap(),
            rule
        );
        assert_eq!(
            format!("{}", rule.human(&CalLocaleEn)),
            "Yearly, in January, on Sun, Mon, Tue, Wed, Thu, Fri, and Sat".to_string()
        );
    }

    fn ny_datetime(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        min: u32,
        sec: u32,
    ) -> DateTime<Utc> {
        chrono_tz::America::New_York
            .with_ymd_and_hms(year, month, day, hour, min, sec)
            .unwrap()
            .naive_local()
            .and_utc()
    }

    fn berlin_datetime(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        min: u32,
        sec: u32,
    ) -> DateTime<Utc> {
        chrono_tz::Europe::Berlin
            .with_ymd_and_hms(year, month, day, hour, min, sec)
            .unwrap()
            .naive_local()
            .and_utc()
    }

    #[test]
    fn range_with_count() {
        let start = ny_datetime(1997, 9, 2, 9, 0, 0);
        let rrule = "FREQ=DAILY;COUNT=3".parse::<CalRRule>().unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Daily\nRepeats 3 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(20),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 3, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 4, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_with_count_dtstart() {
        let dtstart = ny_datetime(1997, 9, 2, 9, 0, 0);
        let start = ny_datetime(1997, 9, 4, 9, 0, 0);
        let rrule = "FREQ=DAILY;COUNT=5".parse::<CalRRule>().unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Daily\nRepeats 5 times".to_string()
        );
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(20),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 4, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 5, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 6, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_with_until() {
        let start = ny_datetime(1997, 10, 25, 9, 0, 0);
        let rrule = "FREQ=DAILY;UNTIL=19971027T000000Z"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Daily\nRepeats until October 27, 1997".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(20),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 25, 9, 0, 0)); // EDT
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 26, 9, 0, 0)); // EST
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_every_other_day() {
        let start = ny_datetime(1997, 10, 25, 9, 0, 0);
        let rrule = "FREQ=DAILY;INTERVAL=2".parse::<CalRRule>().unwrap();
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(10),
        );
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Every 2 days".to_string()
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 25, 9, 0, 0)); // EDT
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 27, 9, 0, 0)); // EST
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 29, 9, 0, 0)); // EST
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 31, 9, 0, 0)); // EST
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 11, 2, 9, 0, 0)); // EST
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_every_10_days() {
        let start = ny_datetime(1997, 9, 2, 9, 0, 0);
        let rrule = "FREQ=DAILY;INTERVAL=10;COUNT=5"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Every 10 days\nRepeats 5 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(100),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 12, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 22, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 12, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_weekly() {
        let start = ny_datetime(1997, 9, 2, 9, 0, 0);
        let rrule = "FREQ=WEEKLY;COUNT=10".parse::<CalRRule>().unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Weekly\nRepeats 10 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::weeks(5),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 9, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 16, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 23, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 30, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_every_monday() {
        let start = ny_datetime(2024, 9, 2, 9, 0, 0);
        let rrule = "FREQ=DAILY;COUNT=5;BYDAY=MO".parse::<CalRRule>().unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Daily, on Mon\nRepeats 5 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::weeks(8),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 9, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 16, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 23, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 30, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_hour_min_sec_limit() {
        let start = ny_datetime(2024, 9, 2, 9, 0, 0);
        let rrule = "FREQ=SECONDLY;COUNT=5;BYHOUR=10,12;BYMINUTE=20,30,40;BYSECOND=10"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Secondly, at hour(s) 10 and 12, at minute(s) 20, 30, and 40, at second(s) 10\nRepeats 5 times"
                .to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::weeks(4),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 2, 10, 20, 10));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 2, 10, 30, 10));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 2, 10, 40, 10));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 2, 12, 20, 10));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 2, 12, 30, 10));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_monthday_limit() {
        let start = ny_datetime(2024, 9, 2, 9, 0, 0);
        let rrule = "FREQ=DAILY;COUNT=7;BYMONTHDAY=3,10,-1"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Daily, on the 3rd, 10th, and last day of the month\nRepeats 7 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::weeks(12),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 3, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 10, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 30, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 10, 3, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 10, 10, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 10, 31, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 11, 3, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_yearday_limit() {
        let start = ny_datetime(2023, 9, 2, 9, 0, 0);
        let rrule = "FREQ=HOURLY;COUNT=4;BYYEARDAY=2,35,-10;BYHOUR=12"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Hourly, on the 2nd, 35th, and 10th to last day of the year, at hour(s) 12\nRepeats 4 times"
                .to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(500),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 12, 22, 12, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 1, 2, 12, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 2, 4, 12, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 12, 22, 12, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn weekly_byday_date_value_mismatch_regression() {
        // Simulates:
        // DTSTART;VALUE=DATE:20260117 (Saturday)
        // RRULE:FREQ=WEEKLY;INTERVAL=1;BYDAY=SU
        // Europe/Berlin timezone

        let dtstart = berlin_datetime(2026, 1, 17, 0, 0, 0); // Saturday

        let rrule = "FREQ=WEEKLY;INTERVAL=1;BYDAY=SU"
            .parse::<CalRRule>()
            .unwrap();

        let mut iter = rrule.dates_between(
            dtstart,
            None, // date-only semantics -> no duration
            dtstart,
            dtstart + Duration::weeks(4),
        );

        // First expected occurrence is Sunday 2026-01-18
        assert_eq!(iter.next(), Some(berlin_datetime(2026, 1, 18, 0, 0, 0)));
    }

    #[test]
    fn range_by_min_and_sec_expand() {
        let start = ny_datetime(2023, 9, 2, 9, 0, 0);
        let rrule = "FREQ=HOURLY;COUNT=8;BYMINUTE=4,5;BYSECOND=10,20,30"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Hourly, at minute(s) 4 and 5, at second(s) 10, 20, and 30\nRepeats 8 times"
                .to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(1),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 4, 10));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 4, 20));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 4, 30));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 5, 10));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 5, 20));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 2, 9, 5, 30));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 2, 10, 4, 10));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 2, 10, 4, 20));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_hour_expand() {
        let start = ny_datetime(2023, 9, 2, 9, 0, 0);
        let rrule = "FREQ=DAILY;COUNT=5;BYHOUR=4,8".parse::<CalRRule>().unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Daily, at hour(s) 4 and 8\nRepeats 5 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(5),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 3, 4, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 3, 8, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 4, 4, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 4, 8, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 5, 4, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_monthday_expand() {
        let start = ny_datetime(2023, 9, 2, 9, 0, 0);
        let rrule = "FREQ=MONTHLY;COUNT=5;BYMONTHDAY=1,-1"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Monthly, on the 1st and last day of the month\nRepeats 5 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(100),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 9, 30, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 10, 1, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 10, 31, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 11, 1, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 11, 30, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_month_expand() {
        let start = ny_datetime(2023, 9, 2, 9, 0, 0);
        let rrule = "FREQ=YEARLY;COUNT=5;BYMONTH=10,11"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Yearly, in October and November\nRepeats 5 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(1000),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 10, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2023, 11, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 10, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 11, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2025, 10, 2, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_day_weekly() {
        let start = ny_datetime(2024, 9, 2, 9, 0, 0);
        let rrule = "FREQ=WEEKLY;COUNT=6;BYDAY=MO,2TU"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Weekly, on Mon and 2nd Tue\nRepeats 6 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(1000),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 3, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 9, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 10, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 16, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 17, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_day_weekly_with_interval() {
        let start = ny_datetime(1997, 9, 3, 9, 0, 0);
        let rrule = "FREQ=WEEKLY;INTERVAL=2;COUNT=6;BYDAY=TU,TH"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Every 2 weeks, on Tue and Thu\nRepeats 6 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(1000),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 4, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 16, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 18, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 30, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 14, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_day_monthly() {
        let start = ny_datetime(2024, 9, 2, 9, 0, 0);
        let rrule = "FREQ=MONTHLY;COUNT=6;BYDAY=MO,2TU,-1WE"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Monthly, on Mon, 2nd Tue, and last Wed\nRepeats 6 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(1000),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 2, 9, 0, 0)); // MO
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 9, 9, 0, 0)); // MO
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 10, 9, 0, 0)); // 2TU
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 16, 9, 0, 0)); // MO
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 23, 9, 0, 0)); // MO
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 25, 9, 0, 0)); // -1WE
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_day_yearly_by_month() {
        let start = ny_datetime(2024, 9, 2, 9, 0, 0);
        let rrule = "FREQ=YEARLY;COUNT=6;BYMONTH=9;BYDAY=MO,2TU,-1WE"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Yearly, in September, on Mon, 2nd Tue, and last Wed\nRepeats 6 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(1000),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 2, 9, 0, 0)); // MO
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 9, 9, 0, 0)); // MO
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 10, 9, 0, 0)); // 2TU
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 16, 9, 0, 0)); // MO
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 23, 9, 0, 0)); // MO
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 9, 25, 9, 0, 0)); // -1WE
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_day_yearly() {
        let start = ny_datetime(2024, 9, 2, 9, 0, 0);
        let rrule = "FREQ=YEARLY;COUNT=6;BYDAY=5MO,-1FR"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Yearly, on 5th Mon and last Fri\nRepeats 6 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::days(2000),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 12, 27, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2025, 2, 3, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2025, 12, 26, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2026, 2, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2026, 12, 25, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2027, 2, 1, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_every_day_in_january() {
        let start = ny_datetime(1998, 1, 1, 9, 0, 0);
        let rrule = "FREQ=YEARLY;COUNT=5;BYMONTH=1;BYDAY=SU,MO,TU,WE,TH,FR,SA"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Yearly, in January, on Sun, Mon, Tue, Wed, Thu, Fri, and Sat\nRepeats 5 times"
                .to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::weeks(4),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1998, 1, 1, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1998, 1, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1998, 1, 3, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1998, 1, 4, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1998, 1, 5, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_every_other_week() {
        let start = ny_datetime(1997, 9, 2, 9, 0, 0);
        let rrule = "FREQ=WEEKLY;COUNT=5;INTERVAL=2"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Every 2 weeks\nRepeats 5 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::weeks(12),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 16, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 30, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 14, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 10, 28, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_every_other_month() {
        let start = ny_datetime(1997, 9, 7, 9, 0, 0);
        let rrule = "FREQ=MONTHLY;INTERVAL=2;COUNT=5;BYDAY=1SU,-1SU"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Every 2 months, on 1st Sun and last Sun\nRepeats 5 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::weeks(100),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 7, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 28, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 11, 2, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 11, 30, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1998, 1, 4, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_every_18_months() {
        let start = ny_datetime(1997, 9, 7, 9, 0, 0);
        let rrule = "FREQ=MONTHLY;INTERVAL=18;COUNT=5;BYMONTHDAY=10,11,15"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Every 18 months, on the 10th, 11th, and 15th day of the month\nRepeats 5 times"
                .to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::weeks(1000),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 10, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 11, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 9, 15, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1999, 3, 10, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1999, 3, 11, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_with_wkst() {
        let start = ny_datetime(1997, 8, 5, 9, 0, 0);
        let rrule = "FREQ=WEEKLY;INTERVAL=2;COUNT=4;BYDAY=TU,SU;WKST=SU"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Every 2 weeks, on Tue and Sun\nRepeats 4 times".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::weeks(1000),
        );
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 8, 5, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 8, 17, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 8, 19, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 8, 31, 9, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn range_by_day_dst_change() {
        let start = chrono_tz::Europe::Berlin
            .with_ymd_and_hms(2025, 3, 24, 10, 0, 0)
            .unwrap();
        let rrule = "FREQ=WEEKLY;INTERVAL=1;BYDAY=MO;WKST=MO"
            .parse::<CalRRule>()
            .unwrap();
        assert_eq!(
            format!("{}", rrule.human(&CalLocaleEn)),
            "Weekly, on Mon".to_string()
        );
        let mut iter = rrule.dates_between(
            start,
            Some(Duration::hours(1)),
            start,
            start + Duration::weeks(2),
        );
        assert_eq!(iter.next().unwrap(), berlin_datetime(2025, 3, 24, 10, 0, 0));
        assert_eq!(iter.next().unwrap(), berlin_datetime(2025, 3, 31, 10, 0, 0));
        assert_eq!(iter.next().unwrap(), berlin_datetime(2025, 4, 7, 10, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn bysetpos_requires_other_by() {
        let result = "FREQ=MONTHLY;BYSETPOS=1".parse::<CalRRule>();
        assert!(result.is_err());
    }

    #[test]
    fn bysetpos_range_is_validated() {
        let result = "FREQ=MONTHLY;BYDAY=MO;BYSETPOS=0".parse::<CalRRule>();
        assert!(result.is_err());
        let result = "FREQ=MONTHLY;BYDAY=MO;BYSETPOS=367".parse::<CalRRule>();
        assert!(result.is_err());
    }

    fn collect_instances(
        rrule: &CalRRule,
        dtstart: DateTime<Utc>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: usize,
    ) -> Vec<DateTime<Utc>> {
        let mut iter = rrule.dates_between(dtstart, Some(Duration::hours(1)), start, end);
        let mut res = Vec::new();
        while res.len() < limit {
            match iter.next() {
                Some(item) => res.push(item),
                None => break,
            }
        }
        res
    }

    #[test]
    fn bysetpos_third_instance_in_month_rfc_example() {
        let dtstart = ny_datetime(1997, 9, 4, 9, 0, 0);
        let rrule = "FREQ=MONTHLY;COUNT=3;BYDAY=TU,WE,TH;BYSETPOS=3"
            .parse::<CalRRule>()
            .unwrap();
        let res = collect_instances(&rrule, dtstart, dtstart, dtstart + Duration::days(120), 10);
        assert_eq!(
            res,
            vec![
                ny_datetime(1997, 9, 4, 9, 0, 0),
                ny_datetime(1997, 10, 7, 9, 0, 0),
                ny_datetime(1997, 11, 6, 9, 0, 0)
            ]
        );
    }

    #[test]
    fn bysetpos_second_to_last_weekday_rfc_example() {
        let dtstart = ny_datetime(1997, 9, 29, 9, 0, 0);
        let rrule = "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-2"
            .parse::<CalRRule>()
            .unwrap();
        let res = collect_instances(
            &rrule,
            dtstart,
            dtstart,
            ny_datetime(1998, 3, 31, 23, 59, 59),
            10,
        );
        assert_eq!(
            res,
            vec![
                ny_datetime(1997, 9, 29, 9, 0, 0),
                ny_datetime(1997, 10, 30, 9, 0, 0),
                ny_datetime(1997, 11, 27, 9, 0, 0),
                ny_datetime(1997, 12, 30, 9, 0, 0),
                ny_datetime(1998, 1, 29, 9, 0, 0),
                ny_datetime(1998, 2, 26, 9, 0, 0),
                ny_datetime(1998, 3, 30, 9, 0, 0)
            ]
        );
    }

    #[test]
    fn bysetpos_out_of_range_yields_no_instances() {
        let dtstart = ny_datetime(1997, 9, 1, 9, 0, 0);
        let rrule = "FREQ=MONTHLY;BYDAY=MO;BYSETPOS=10"
            .parse::<CalRRule>()
            .unwrap();
        let res = collect_instances(
            &rrule,
            dtstart,
            dtstart,
            ny_datetime(1997, 12, 31, 23, 59, 59),
            10,
        );
        assert!(res.is_empty());
    }

    #[test]
    fn bysetpos_deduplicates_overlapping_positions() {
        let dtstart = ny_datetime(1997, 9, 1, 9, 0, 0);
        let rrule = "FREQ=MONTHLY;COUNT=2;BYMONTHDAY=1;BYSETPOS=1,-1"
            .parse::<CalRRule>()
            .unwrap();
        let res = collect_instances(
            &rrule,
            dtstart,
            dtstart,
            ny_datetime(1997, 11, 30, 23, 59, 59),
            10,
        );
        assert_eq!(
            res,
            vec![
                ny_datetime(1997, 9, 1, 9, 0, 0),
                ny_datetime(1997, 10, 1, 9, 0, 0)
            ]
        );
    }

    #[test]
    fn bysetpos_respects_chronological_order() {
        let dtstart = ny_datetime(1997, 9, 1, 9, 0, 0);
        let rrule = "FREQ=MONTHLY;COUNT=2;BYDAY=MO,WE;BYSETPOS=2,1"
            .parse::<CalRRule>()
            .unwrap();
        let res = collect_instances(
            &rrule,
            dtstart,
            dtstart,
            ny_datetime(1997, 9, 30, 23, 59, 59),
            10,
        );
        assert_eq!(
            res,
            vec![
                ny_datetime(1997, 9, 1, 9, 0, 0),
                ny_datetime(1997, 9, 3, 9, 0, 0)
            ]
        );
    }

    #[test]
    fn rrule_byyearday_rfc_example() {
        let dtstart = ny_datetime(1997, 1, 1, 9, 0, 0);
        let rrule = "FREQ=YEARLY;INTERVAL=3;COUNT=10;BYYEARDAY=1,100,200"
            .parse::<CalRRule>()
            .unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            ny_datetime(2006, 1, 2, 9, 0, 0),
        );

        let expected = vec![
            ny_datetime(1997, 1, 1, 9, 0, 0),
            ny_datetime(1997, 4, 10, 9, 0, 0),
            ny_datetime(1997, 7, 19, 9, 0, 0),
            ny_datetime(2000, 1, 1, 9, 0, 0),
            ny_datetime(2000, 4, 9, 9, 0, 0),
            ny_datetime(2000, 7, 18, 9, 0, 0),
            ny_datetime(2003, 1, 1, 9, 0, 0),
            ny_datetime(2003, 4, 10, 9, 0, 0),
            ny_datetime(2003, 7, 19, 9, 0, 0),
            ny_datetime(2006, 1, 1, 9, 0, 0),
        ];

        for exp in expected {
            assert_eq!(iter.next().unwrap(), exp);
        }
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn rrule_byweekno_rfc_example() {
        let dtstart = ny_datetime(1997, 5, 12, 9, 0, 0);
        let rrule = "FREQ=YEARLY;BYWEEKNO=20;BYDAY=MO"
            .parse::<CalRRule>()
            .unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            ny_datetime(2000, 1, 2, 9, 0, 0),
        );

        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 5, 12, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1998, 5, 11, 9, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1999, 5, 17, 9, 0, 0));
    }

    #[test]
    fn rrule_hourly_byyearday_does_not_duplicate_or_reorder() {
        let dtstart = ny_datetime(2024, 1, 1, 12, 0, 0);
        let rrule = "FREQ=HOURLY;BYYEARDAY=2,35;BYHOUR=12"
            .parse::<CalRRule>()
            .unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            ny_datetime(2024, 3, 1, 0, 0, 0),
        );

        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 1, 2, 12, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(2024, 2, 4, 12, 0, 0));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn rrule_yearly_byyearday_expands_multiple_hours() {
        let dtstart = ny_datetime(1997, 1, 1, 9, 0, 0);
        let rrule = "FREQ=YEARLY;COUNT=4;BYYEARDAY=1;BYHOUR=10,12"
            .parse::<CalRRule>()
            .unwrap();
        let mut iter = rrule.dates_between(
            dtstart,
            Some(Duration::hours(1)),
            dtstart,
            ny_datetime(1999, 1, 2, 0, 0, 0),
        );

        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 1, 1, 10, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1997, 1, 1, 12, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1998, 1, 1, 10, 0, 0));
        assert_eq!(iter.next().unwrap(), ny_datetime(1998, 1, 1, 12, 0, 0));
        assert_eq!(iter.next(), None);
    }

    // --- DST-aware overlap check in finalize_candidates ---

    /// Verifies that the range overlap check in `finalize_candidates` uses wall-clock end times.
    ///
    /// Europe/Berlin springs forward on 2025-03-30 at 02:00 → 03:00. An occurrence starting at
    /// 01:00 CET with a 3-hour duration ends at 04:00 CEST (wall-clock). A naive absolute
    /// addition would compute 05:00 CEST instead (one hour too late).
    ///
    /// The query window [04:30 CEST, 05:30 CEST) should NOT be overlapped by this occurrence,
    /// because 04:00 is before 04:30. The buggy absolute-time computation would incorrectly
    /// include it (05:00 falls inside the window).
    #[test]
    fn finalize_candidates_dst_overlap_uses_wall_clock_end() {
        // DTSTART: 2025-03-30 01:00 CET (one hour before spring-forward)
        let dtstart = berlin_datetime(2025, 3, 30, 1, 0, 0);

        // A non-recurrent rule (single occurrence via COUNT=1) so that exactly one candidate
        // is produced at dtstart itself.
        let rrule: CalRRule = "FREQ=DAILY;COUNT=1".parse().unwrap();

        // Duration: 3 hours. Wall-clock end = 04:00 CEST; absolute end = 05:00 CEST.
        let duration = Duration::hours(3);

        // Query window that only the buggy absolute end (05:00) would overlap.
        let window_start = berlin_datetime(2025, 3, 30, 4, 30, 0);
        let window_end = berlin_datetime(2025, 3, 30, 5, 30, 0);

        let mut iter = rrule.dates_between(dtstart, Some(duration), window_start, window_end);

        // the occurrence ends at 04:00 CEST which is before the window start (04:30 CEST), so it
        // must NOT appear in the results.
        assert_eq!(
            iter.next(),
            None,
            "occurrence must not overlap a window that starts after its wall-clock end"
        );
    }
}
