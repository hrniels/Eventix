use chrono::offset::LocalResult;
use chrono::{
    DateTime, Datelike, Duration, Month, Months, NaiveDateTime, TimeDelta, Timelike, Weekday,
};
use chrono_tz::Tz;
use formatx::formatx;
use itertools::Itertools;
use std::fmt;
use std::str::FromStr;

use crate::objects::{CalDate, CalLocale, CalLocaleEn};
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
    pub fn matches(&self, date: DateTime<Tz>, rrule: &CalRRule) -> bool {
        match self.nth {
            None => self.day == date.weekday(),
            Some((n, side)) => {
                // offset within the month
                if rrule.freq == CalRRuleFreq::Monthly
                    || (rrule.freq == CalRRuleFreq::Yearly && rrule.by_month.is_some())
                {
                    match side {
                        CalRRuleSide::Start => util::nth_weekday_of_month_front(date, self.day, n),
                        CalRRuleSide::End => util::nth_weekday_of_month_back(date, self.day, n),
                    }
                    .map(|d| d == date.date_naive())
                    .unwrap_or(false)
                }
                // offset within the year
                else if rrule.freq == CalRRuleFreq::Yearly {
                    match side {
                        CalRRuleSide::Start => util::nth_weekday_of_year_front(date, self.day, n),
                        CalRRuleSide::End => util::nth_weekday_of_year_back(date, self.day, n),
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
        if let Some((num, side)) = self.wday.nth {
            write!(
                f,
                "{} {}",
                self.locale.nth_day(num as u64, side == CalRRuleSide::Start),
                wday
            )
        } else {
            write!(f, "{}", self.locale.translate(&wday))
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct DayDesc {
    num: u16,
    side: CalRRuleSide,
}

impl DayDesc {
    #[cfg(test)]
    pub fn new(num: u16, side: CalRRuleSide) -> Self {
        Self { num, side }
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
    count: Option<u8>,
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

fn next_date(date: DateTime<Tz>, freq: CalRRuleFreq, interval: u32) -> Option<DateTime<Tz>> {
    // we basically want to ignore DST here, in the sense that all recurrences of an event
    // that started at 9:00 AM should always be at 9:00 AM as well, regardless of whether
    // DST is on or off. For that reason, we build a NaiveDateTime from the date in the
    // selected timezone, advance it accordingly, and turn it back into a DateTime.
    for i in 1.. {
        let next = freq.advance(date.naive_local(), interval * i)?;
        // if the date is not representable in our timezone, just skip it
        if let LocalResult::Single(localdate) = next.and_local_timezone(date.timezone()) {
            return Some(localdate);
        }
    }
    unreachable!();
}

/// Iterator for [`CalRRule`].
pub struct RecurIterator<'a> {
    rrule: &'a CalRRule,
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    dtstart: DateTime<Tz>,
    dtdur: Option<Duration>,
    date: DateTime<Tz>,
    until: DateTime<Tz>,
    count: usize,
    interval: u32,
    last: Vec<DateTime<Tz>>,
    last_pos: usize,
}

impl Iterator for RecurIterator<'_> {
    type Item = DateTime<Tz>;

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
    pub fn count(&self) -> Option<u8> {
        self.count
    }
    /// Sets the number of recurrences (COUNT).
    ///
    /// If set to `None`, the recurrence occurs indefinitely.
    pub fn set_count(&mut self, count: u8) {
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
    pub fn dates_between(
        &self,
        dtstart: DateTime<Tz>,
        dtdur: Option<Duration>,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> RecurIterator<'_> {
        let interval = self.interval.unwrap_or(1) as u32;
        // go one interval further to ensure that we do not miss an occurrence. for example, if we
        // want to see all occurrences until December 20th of a monthly event starting at 25th of
        // January, we will not consider the December as the 25th is already out of range. going
        // one interval further means that we will consider the December and might set the day to
        // something else, which might indeed be in the range.
        let beyond_end = next_date(end, self.freq, interval).unwrap_or(end);
        let until = if let Some(ref until) = self.until {
            until.as_end_with_tz(&start.timezone()).min(beyond_end)
        } else {
            beyond_end
        };

        assert!(self.by_set_pos.is_none(), "BYSETPOS is not supported");

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

    fn limited(&self, date: DateTime<Tz>) -> bool {
        if let Some(by_month) = &self.by_month
            && self.freq <= CalRRuleFreq::Monthly
            && !by_month.contains(&(date.month() as u8))
        {
            return true;
        }

        if let Some(by_yday) = &self.by_year_day
            && self.freq <= CalRRuleFreq::Hourly
            && !by_yday.iter().any(|yd| match yd.side {
                CalRRuleSide::Start => yd.num as u32 == util::year_day(date),
                CalRRuleSide::End => {
                    let days = util::year_days(date.year());
                    days - (yd.num - 1) as u32 == util::year_day(date)
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
        dtstart: DateTime<Tz>,
        dtdur: Option<Duration>,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        date: DateTime<Tz>,
        count: &mut usize,
    ) -> Option<Vec<DateTime<Tz>>> {
        let months = [date.month() as u8];
        let mut months = months.as_slice();
        let mut mon_days = vec![date.day() as u16];
        let hours = [date.hour() as u8];
        let mut hours = hours.as_slice();
        let mins = [date.minute() as u8];
        let mut mins = mins.as_slice();
        let secs = [date.second() as u8];
        let mut secs = secs.as_slice();

        if self.by_year_day.is_some() && self.freq > CalRRuleFreq::Monthly {
            unimplemented!("BYYEARDAY expansion is not supported");
        }
        if self.by_week_no.is_some() && self.freq > CalRRuleFreq::Monthly {
            unimplemented!("BYWEEKNO expansion is not supported");
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

        if self.freq >= CalRRuleFreq::Weekly
            && let Some(by_day) = self.by_day.as_ref()
        {
            // Define a unified period window: [period_start, next_period_start)
            let period_start = match self.freq {
                CalRRuleFreq::Weekly => util::week_start(date, self.week_start),
                CalRRuleFreq::Monthly => date.with_day(1)?,
                _ => {
                    // Yearly
                    date.with_month(1)?.with_day(1)?
                }
            };

            let period_end = match self.freq {
                CalRRuleFreq::Weekly => {
                    // Advance exactly one week using recurrence-aware logic (DST-safe)
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

            let mut res = vec![];
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
                                    && ndate >= dtstart
                                {
                                    if util::date_ranges_overlap(
                                        ndate,
                                        ndate + dtdur.unwrap_or(Duration::zero()),
                                        start,
                                        end,
                                    ) {
                                        res.push(ndate.with_timezone(&start.timezone()));
                                    }
                                    *count += 1;
                                }

                                if let Some(rcount) = self.count
                                    && *count >= rcount as usize
                                {
                                    if !res.is_empty() {
                                        return Some(res);
                                    }
                                    return None;
                                }
                            }
                        }
                    }
                }
                vcur = next_date(vcur, CalRRuleFreq::Daily, 1).unwrap();
            }
            return Some(res);
        }

        let mut res = vec![];
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
                                && ndate >= dtstart
                            {
                                if util::date_ranges_overlap(
                                    ndate,
                                    ndate + dtdur.unwrap_or(Duration::zero()),
                                    start,
                                    end,
                                ) {
                                    res.push(ndate.with_timezone(&start.timezone()));
                                }
                                *count += 1;
                            }

                            if let Some(rcount) = self.count
                                && *count >= rcount as usize
                            {
                                if !res.is_empty() {
                                    return Some(res);
                                }
                                return None;
                            }
                        }
                    }
                }
            }
        }
        Some(res)
    }

    /// Returns a human-readable representation of this recurrence rule.
    pub fn human<'l>(&self, locale: &'l dyn CalLocale) -> RRuleHuman<'_, 'l> {
        RRuleHuman {
            rrule: self,
            locale,
        }
    }
}

/// Implements [`Display`](fmt::Display) to create a human-readable representation of a
/// [`CalRRule`].
///
/// For example, it could say "Every 2 years".
pub struct RRuleHuman<'r, 'l> {
    rrule: &'r CalRRule,
    locale: &'l dyn CalLocale,
}

impl fmt::Display for RRuleHuman<'_, '_> {
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

        let locale = &CalLocaleEn;

        write_list(self.by_second.as_ref().map(|v| v.iter()), "BYSECOND", f)?;
        write_list(self.by_minute.as_ref().map(|v| v.iter()), "BYMINUTE", f)?;
        write_list(self.by_hour.as_ref().map(|v| v.iter()), "BYHOUR", f)?;
        write_list(self.by_day.as_ref().map(|v| v.iter()), "BYDAY", f)?;
        write_list(
            self.by_mon_day
                .as_ref()
                .map(|v| v.iter().map(|d| d.human(locale))),
            "BYMONDAY",
            f,
        )?;
        write_list(
            self.by_year_day
                .as_ref()
                .map(|v| v.iter().map(|d| d.human(locale))),
            "BYYEARDAY",
            f,
        )?;
        write_list(
            self.by_week_no
                .as_ref()
                .map(|v| v.iter().map(|d| d.human(locale))),
            "BYWEEKNO",
            f,
        )?;
        write_list(self.by_month.as_ref().map(|v| v.iter()), "BYMONTH", f)?;
        write_list(
            self.by_set_pos
                .as_ref()
                .map(|v| v.iter().map(|d| d.human(locale))),
            "BYSETPOS",
            f,
        )?;
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
        Ok(rrule)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc, Weekday};

    use crate::objects::date::CalDateTime;

    use super::*;

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
        let mut rule = CalRRule::default();
        rule.freq = CalRRuleFreq::Daily;
        rule.count = Some(10);
        assert_eq!("FREQ=DAILY;COUNT=10".parse::<CalRRule>().unwrap(), rule);
        assert_eq!(
            format!("{}", rule.human(&CalLocaleEn)),
            "Daily\nRepeats 10 times".to_string()
        );
    }

    #[test]
    fn parse_recur_interval() {
        let mut rule = CalRRule::default();
        rule.freq = CalRRuleFreq::Monthly;
        rule.interval = Some(2);
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
    fn parse_recur_until() {
        let mut rule = CalRRule::default();
        rule.freq = CalRRuleFreq::Daily;
        rule.until = Some(CalDate::DateTime(CalDateTime::Utc(
            Utc.with_ymd_and_hms(1997, 12, 24, 0, 0, 0).unwrap(),
        )));
        assert_eq!(
            "FREQ=DAILY;UNTIL=19971224T000000Z"
                .parse::<CalRRule>()
                .unwrap(),
            rule
        );
    }

    #[test]
    fn parse_recur_by() {
        let mut rule = CalRRule::default();
        rule.freq = CalRRuleFreq::Yearly;
        rule.by_month = Some(vec![1]);
        rule.by_set_pos = Some(vec![
            DayDesc::new(2, CalRRuleSide::Start),
            DayDesc::new(5, CalRRuleSide::Start),
        ]);
        rule.by_day = Some(vec![
            CalWDayDesc::new(Weekday::Sun, None),
            CalWDayDesc::new(Weekday::Mon, None),
            CalWDayDesc::new(Weekday::Tue, None),
            CalWDayDesc::new(Weekday::Wed, None),
            CalWDayDesc::new(Weekday::Thu, None),
            CalWDayDesc::new(Weekday::Fri, None),
            CalWDayDesc::new(Weekday::Sat, None),
        ]);

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

    fn ny_datetime(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> DateTime<Tz> {
        chrono_tz::America::New_York
            .with_ymd_and_hms(year, month, day, hour, min, sec)
            .unwrap()
    }

    fn berlin_datetime(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        min: u32,
        sec: u32,
    ) -> DateTime<Tz> {
        chrono_tz::Europe::Berlin
            .with_ymd_and_hms(year, month, day, hour, min, sec)
            .unwrap()
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
        let start = berlin_datetime(2025, 3, 24, 10, 0, 0);
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
}
