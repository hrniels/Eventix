// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cmp::Ordering,
    fmt,
    ops::{Add, Deref, Sub},
    str::FromStr,
    sync::Arc,
};

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::parser::{Parameter, ParseError, Property};

use super::{CalCompType, CalendarTimeZoneResolver};

/// A reusable calendar-bound context for resolving unresolved dates.
///
/// The contained resolver is cached by the owning calendar and shared via [`Arc`]. Methods that
/// resolve floating values or plain dates accept an explicit fallback timezone when needed.
#[derive(Clone, Debug)]
pub struct DateContext {
    resolver: Arc<CalendarTimeZoneResolver>,
}

impl DateContext {
    /// Creates a new resolution context from the given resolver.
    pub fn new(resolver: Arc<CalendarTimeZoneResolver>) -> Self {
        Self { resolver }
    }

    /// Creates a context that uses only the system timezone database.
    pub fn system() -> Self {
        Self::new(Arc::new(CalendarTimeZoneResolver::default()))
    }

    /// Returns the shared resolver used by this context.
    pub fn resolver(&self) -> &CalendarTimeZoneResolver {
        &self.resolver
    }

    /// Returns a bound view for the given unresolved calendar date.
    pub fn date<'a>(&'a self, raw: &'a CalDate) -> BoundCalDate<'a> {
        BoundCalDate::new(raw, self)
    }

    /// Resolves the given calendar date as the start of an event.
    pub fn resolve_date_start(&self, date: &CalDate, fallback: &Tz) -> ResolvedDateTime {
        self.resolver.resolve_date_start(date, fallback)
    }

    /// Resolves the given calendar date as the inclusive end of an event or TODO.
    pub fn resolve_date_end(&self, date: &CalDate, fallback: &Tz) -> ResolvedDateTime {
        self.resolver.resolve_date_end(date, fallback)
    }

    /// Resolves the given calendar datetime into a concrete instant.
    pub fn resolve_datetime(&self, dt: &CalDateTime, fallback: &Tz) -> ResolvedDateTime {
        self.resolver.resolve_datetime(dt, fallback)
    }

    /// Validates the given calendar date using this context.
    pub fn validate_date(&self, date: &CalDate, local_tz: &Tz) -> Result<(), ParseError> {
        self.resolver.validate_date(date, local_tz)
    }

    /// Validates the given calendar datetime using this context.
    pub fn validate_datetime(&self, dt: &CalDateTime, local_tz: &Tz) -> Result<(), ParseError> {
        self.resolver.validate_datetime(dt, local_tz)
    }

    /// Converts the given calendar date into a UTC-normalized form.
    ///
    /// Plain `DATE` values keep their calendar-day semantics. `DATE-TIME` values are resolved via
    /// this context and returned as UTC datetimes.
    pub fn date_to_utc(&self, date: &CalDate, fallback: &Tz) -> CalDate {
        match date {
            CalDate::Date(day, ty) => CalDate::Date(*day, *ty),
            CalDate::DateTime(_) => CalDate::from(self.resolve_date_start(date, fallback)),
        }
    }

    /// Converts the given calendar datetime into a UTC datetime using this context.
    pub fn datetime_to_utc(&self, dt: &CalDateTime, fallback: &Tz) -> CalDateTime {
        CalDateTime::Utc(self.resolve_datetime(dt, fallback).with_timezone(&Utc))
    }
}

/// A calendar date paired with a calendar-bound resolution context.
#[derive(Clone, Copy, Debug)]
pub struct BoundCalDate<'a> {
    raw: &'a CalDate,
    ctx: &'a DateContext,
}

impl<'a> BoundCalDate<'a> {
    /// Creates a new bound view for the given unresolved date.
    pub fn new(raw: &'a CalDate, ctx: &'a DateContext) -> Self {
        Self { raw, ctx }
    }

    /// Returns the underlying unresolved calendar date.
    pub fn raw(&self) -> &'a CalDate {
        self.raw
    }

    /// Returns the context used to resolve this date.
    pub fn context(&self) -> &'a DateContext {
        self.ctx
    }

    /// Resolves this date as the start of an event.
    pub fn resolved_start(&self, fallback: &Tz) -> ResolvedDateTime {
        self.ctx.resolve_date_start(self.raw, fallback)
    }

    /// Resolves this date as the inclusive end of an event or TODO.
    pub fn resolved_end(&self, fallback: &Tz) -> ResolvedDateTime {
        self.ctx.resolve_date_end(self.raw, fallback)
    }

    /// Resolves the start of this date and converts it into the given display timezone.
    pub fn start_in(&self, tz: &Tz) -> DateTime<Tz> {
        self.resolved_start(tz).with_timezone(tz)
    }

    /// Resolves the end of this date and converts it into the given display timezone.
    pub fn end_in(&self, tz: &Tz) -> DateTime<Tz> {
        self.resolved_end(tz).with_timezone(tz)
    }

    /// Validates this date using the resolver captured in the bound context.
    pub fn validate(&self, local_tz: &Tz) -> Result<(), ParseError> {
        self.ctx.validate_date(self.raw, local_tz)
    }

    /// Formats this date when interpreted as the start of an event.
    pub fn fmt_start_in(&self, tz: &Tz) -> String {
        self.fmt_date(self.start_in(tz))
    }

    /// Formats this date when interpreted as the inclusive end of an event or TODO.
    pub fn fmt_end_in(&self, tz: &Tz) -> String {
        self.fmt_date(self.end_in(tz))
    }

    /// Converts this bound date into a UTC-normalized calendar date.
    pub fn to_utc_date(&self, fallback: &Tz) -> CalDate {
        self.ctx.date_to_utc(self.raw, fallback)
    }

    fn fmt_date(&self, dt: DateTime<Tz>) -> String {
        match self.raw {
            CalDate::Date(..) => dt.format("%B %d, %Y").to_string(),
            CalDate::DateTime(_) => dt.format("%A, %B %d, %Y %H:%M").to_string(),
        }
    }
}

/// A fully resolved calendar timestamp with a concrete UTC offset.
///
/// Unlike [`CalDate`] and [`CalDateTime`], this type no longer carries unresolved calendar-local
/// semantics. It represents the concrete instant produced after resolving a local date/time
/// through a timezone definition.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ResolvedDateTime(DateTime<FixedOffset>);

impl ResolvedDateTime {
    /// Creates a resolved timestamp from a concrete fixed-offset datetime.
    pub fn new(dt: DateTime<FixedOffset>) -> Self {
        Self(dt)
    }

    /// Returns the wrapped fixed-offset datetime.
    pub fn into_inner(self) -> DateTime<FixedOffset> {
        self.0
    }
}

impl Deref for ResolvedDateTime {
    type Target = DateTime<FixedOffset>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<DateTime<FixedOffset>> for ResolvedDateTime {
    fn from(value: DateTime<FixedOffset>) -> Self {
        Self(value)
    }
}

impl From<ResolvedDateTime> for DateTime<FixedOffset> {
    fn from(value: ResolvedDateTime) -> Self {
        value.0
    }
}

impl Add<Duration> for ResolvedDateTime {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl Sub<Duration> for ResolvedDateTime {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        Self(self.0 - rhs)
    }
}

impl Sub for ResolvedDateTime {
    type Output = Duration;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0 - rhs.0
    }
}

impl<Tz: TimeZone> PartialEq<DateTime<Tz>> for ResolvedDateTime {
    fn eq(&self, other: &DateTime<Tz>) -> bool {
        self.0.with_timezone(&Utc) == other.with_timezone(&Utc)
    }
}

impl<Tz: TimeZone> PartialEq<ResolvedDateTime> for DateTime<Tz> {
    fn eq(&self, other: &ResolvedDateTime) -> bool {
        other == self
    }
}

impl<Tz: TimeZone> PartialOrd<DateTime<Tz>> for ResolvedDateTime {
    fn partial_cmp(&self, other: &DateTime<Tz>) -> Option<Ordering> {
        self.0
            .with_timezone(&Utc)
            .partial_cmp(&other.with_timezone(&Utc))
    }
}

impl<Tz: TimeZone> PartialOrd<ResolvedDateTime> for DateTime<Tz> {
    fn partial_cmp(&self, other: &ResolvedDateTime) -> Option<Ordering> {
        self.with_timezone(&Utc)
            .partial_cmp(&other.0.with_timezone(&Utc))
    }
}

/// The type of date.
///
/// The iCalendar format has interestingly two different ways to interpret dates of type
/// [`CalDate::Date`]. For events, the end is interpreted as "exclusive", meaning that an event
/// that starts on 2025-02-23 and ends on 2025-02-24 is actually just one day long (the entire
/// 2025-02-23) and ends at the start of 2025-02-24. For TODOs however, the due date is
/// "inclusive". For example, if the due date is 2025-02-23, the TODO is due until the *end* of
/// that day.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum CalDateType {
    /// The date is inclusive, meaning that the event ends *after* that date.
    Inclusive,
    /// The date is exclusive, meaning that the event ends *before* that date.
    Exclusive,
}

impl fmt::Display for CalDateType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FromStr for CalDateType {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Inclusive" => Ok(Self::Inclusive),
            "Exclusive" => Ok(Self::Exclusive),
            _ => Err(ParseError::InvalidDate(s.to_string())),
        }
    }
}

impl From<CalCompType> for CalDateType {
    fn from(value: CalCompType) -> Self {
        match value {
            CalCompType::Event => Self::Exclusive,
            CalCompType::Todo => Self::Inclusive,
        }
    }
}

/// An iCalendar date.
///
/// Dates in iCalendar objects come in two forms: date and datetime. The former specifies a day,
/// whereas the latter specifies a day and a time.
///
/// This is intentionally an unresolved calendar value. Equality, hashing, and ordering are
/// structural and preserve the original iCalendar representation rather than comparing resolved
/// instants. Code that needs calendar-aware timezone resolution must go through [`DateContext`] or
/// [`BoundCalDate`].
#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum CalDate {
    /// Specifies a date.
    ///
    /// As the interpretation depends on the the component type, this variant consists of both the
    /// [`NaiveDate`] holding the day and the [`CalDateType`] holding the type.
    Date(NaiveDate, CalDateType),

    /// Specifies a date and a time.
    DateTime(CalDateTime),
}

impl Serialize for CalDate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{self}"))
    }
}

impl<'de> Deserialize<'de> for CalDate {
    fn deserialize<D>(deserializer: D) -> Result<CalDate, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf = String::deserialize(deserializer)?;
        buf.parse().map_err(serde::de::Error::custom)
    }
}

impl Default for CalDate {
    fn default() -> Self {
        Self::Date(NaiveDate::default(), CalDateType::Inclusive)
    }
}

impl CalDate {
    /// Returns a new [`CalDate::Date`] instance for the given date.
    pub fn new_date(date: NaiveDate, ty: CalDateType) -> Self {
        Self::Date(date, ty)
    }

    /// Returns a new [`CalDate::DateTime`] instance for the current time in UTC.
    pub fn now() -> Self {
        CalDate::DateTime(CalDateTime::Utc(Utc::now()))
    }

    /// Returns the [`NaiveDate`] instance corresponding to this [`CalDate`].
    pub fn as_naive_date(&self) -> NaiveDate {
        match self {
            Self::Date(date, _) => *date,
            Self::DateTime(datetime) => datetime.as_naive_date(),
        }
    }

    /// Builds and returns a [`Property`] for this date.
    pub fn to_prop<N: ToString>(&self, name: N) -> Property {
        match self {
            Self::Date(date, _) => Property::new(
                name,
                vec![Parameter::new("VALUE", "DATE")],
                date.format("%Y%m%d").to_string(),
            ),
            Self::DateTime(datetime) => datetime.to_prop(name),
        }
    }

    /// Validates this date using the given timezone resolver.
    ///
    /// This behaves like [`Self::validate`], but resolves `TZID` values through the provided
    /// resolver so embedded `VTIMEZONE` definitions are taken into account.
    pub fn validate_with(
        &self,
        local_tz: &Tz,
        resolver: &CalendarTimeZoneResolver,
    ) -> Result<(), ParseError> {
        resolver.validate_date(self, local_tz)
    }

    /// Resolves this date as the start of an event using the given timezone resolver.
    ///
    /// Floating values and plain dates use `fallback` when they need a timezone context. `TZID`
    /// values are resolved through `resolver`, which may prefer embedded `VTIMEZONE` data over the
    /// system timezone database.
    pub fn as_start_with_resolver(
        &self,
        fallback: &Tz,
        resolver: &CalendarTimeZoneResolver,
    ) -> ResolvedDateTime {
        resolver.resolve_date_start(self, fallback)
    }

    /// Resolves this date as the end of an event using the given timezone resolver.
    ///
    /// This uses the same inclusive end semantics as [`Self::as_end_with_tz`], but returns a
    /// concrete resolved instant that preserves the final fixed offset chosen by the resolver.
    pub fn as_end_with_resolver(
        &self,
        fallback: &Tz,
        resolver: &CalendarTimeZoneResolver,
    ) -> ResolvedDateTime {
        resolver.resolve_date_end(self, fallback)
    }
}

impl From<DateTime<Tz>> for CalDate {
    fn from(date: DateTime<Tz>) -> Self {
        Self::DateTime(CalDateTime::Timezone(
            date.naive_local(),
            date.timezone().name().to_string(),
        ))
    }
}

impl From<DateTime<FixedOffset>> for CalDate {
    fn from(date: DateTime<FixedOffset>) -> Self {
        Self::DateTime(CalDateTime::Utc(date.with_timezone(&Utc)))
    }
}

impl From<ResolvedDateTime> for CalDate {
    fn from(date: ResolvedDateTime) -> Self {
        Self::from(date.into_inner())
    }
}

impl FromStr for CalDate {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.chars().next() {
            Some('D') => {
                let mut parts = s[1..].splitn(2, ';');
                let d = parts
                    .next()
                    .unwrap()
                    .parse::<NaiveDate>()
                    .map_err(|_| ParseError::MalformedDate(s.to_string()))?;
                let ty = parts
                    .next()
                    .unwrap()
                    .parse::<CalDateType>()
                    .map_err(|_| ParseError::MalformedDate(s.to_string()))?;
                Ok(CalDate::Date(d, ty))
            }
            Some('T') => Ok(CalDate::DateTime(
                s[1..]
                    .parse()
                    .map_err(|_| ParseError::MalformedDate(s.to_string()))?,
            )),
            _ => Err(ParseError::MalformedDate(s.to_string())),
        }
    }
}

impl fmt::Display for CalDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Date(date, ty) => write!(f, "D{};{}", date, ty),
            Self::DateTime(dt) => write!(f, "T{}", dt),
        }
    }
}

/// An iCalendar date in datetime format.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum CalDateTime {
    /// The datetime is floating in the sense that the date/time is always the same, independent of
    /// the timezone of the user. For example, if it was created for 8 AM by a user in UTC, it will
    /// also be displayed as 8 AM for a user in UTC-6.
    Floating(NaiveDateTime),

    /// Datetime in UTC.
    Utc(DateTime<Utc>),

    /// Datetime with a specific timezone.
    Timezone(NaiveDateTime, String),
}

impl CalDateTime {
    /// Returns the [`NaiveDate`] instance corresponding to this [`CalDateTime`].
    pub fn as_naive_date(&self) -> NaiveDate {
        match self {
            Self::Utc(dt) => dt.date_naive(),
            Self::Timezone(dt, _tzid) => dt.date(),
            Self::Floating(dt) => dt.date(),
        }
    }

    /// Returns the [`NaiveTime`] instance corresponding to this [`CalDateTime`].
    pub fn as_naive_time(&self) -> NaiveTime {
        match self {
            Self::Utc(dt) => dt.naive_local().time(),
            Self::Timezone(dt, _tzid) => dt.time(),
            Self::Floating(dt) => dt.time(),
        }
    }

    /// Builds and returns a [`Property`] for this datetime.
    pub fn to_prop<N: ToString>(&self, name: N) -> Property {
        let (params, date) = match self {
            Self::Floating(date) => (vec![], date.format("%Y%m%dT%H%M%S")),
            Self::Utc(dt) => (vec![], dt.format("%Y%m%dT%H%M%SZ")),
            Self::Timezone(dt, tz) => {
                (vec![Parameter::new("TZID", tz)], dt.format("%Y%m%dT%H%M%S"))
            }
        };
        Property::new(name, params, date.to_string())
    }
}

impl FromStr for CalDateTime {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.chars().next() {
            Some('F') => {
                let date = s[1..]
                    .parse::<NaiveDateTime>()
                    .map_err(|_| ParseError::MalformedDate("foo".to_owned() + s))?;
                Ok(CalDateTime::Floating(date))
            }
            Some('U') => {
                let date = s[1..]
                    .parse::<NaiveDateTime>()
                    .map_err(|_| ParseError::MalformedDate(s.to_string()))?;
                Ok(CalDateTime::Utc(date.and_utc()))
            }
            Some('T') => {
                let mut parts = s[1..].splitn(2, ';');
                let tz = parts.next().unwrap().to_string();
                let dt = parts
                    .next()
                    .unwrap()
                    .parse::<NaiveDateTime>()
                    .map_err(|_| ParseError::MalformedDate(s.to_string()))?;
                Ok(CalDateTime::Timezone(dt, tz))
            }
            _ => Err(ParseError::MalformedDate(s.to_string())),
        }
    }
}

impl fmt::Display for CalDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Floating(date) => write!(f, "F{}", date.format("%Y-%m-%dT%H:%M:%S")),
            Self::Utc(datetime) => write!(f, "U{}", datetime.format("%Y-%m-%dT%H:%M:%S")),
            Self::Timezone(datetime, tz) => {
                write!(f, "T{};{}", tz, datetime.format("%Y-%m-%dT%H:%M:%S"))
            }
        }
    }
}

impl TryFrom<Property> for CalDate {
    type Error = ParseError;

    fn try_from(prop: Property) -> Result<Self, Self::Error> {
        let datetime = prop.value();
        if datetime.len() < 8 {
            return Err(ParseError::MalformedDate(datetime.to_string()));
        }

        let year = datetime[0..4].parse::<i32>()?;
        let month = datetime[4..6].parse::<u32>()?;
        let day = datetime[6..8].parse::<u32>()?;

        if datetime.len() == 8 || prop.has_param_value("VALUE", "DATE") {
            let date = NaiveDate::from_ymd_opt(year, month, day)
                .ok_or_else(|| ParseError::InvalidDate(datetime.to_string()))?;
            let ty = if prop.name() == "DUE" {
                CalDateType::Inclusive
            } else {
                CalDateType::Exclusive
            };
            return Ok(CalDate::Date(date, ty));
        }

        if datetime.len() < 15 || &datetime[8..9] != "T" {
            return Err(ParseError::MalformedDate(datetime.to_string()));
        }

        let hour = datetime[9..11].parse::<u32>()?;
        let min = datetime[11..13].parse::<u32>()?;
        let sec = datetime[13..15].parse::<u32>()?;

        let date = NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|d| d.and_hms_opt(hour, min, sec))
            .ok_or_else(|| ParseError::InvalidDate(datetime.to_string()))?;

        let res = if datetime.ends_with('Z') {
            CalDateTime::Utc(date.and_utc())
        } else if let Some(tz) = prop.param("TZID") {
            CalDateTime::Timezone(date, tz.value().clone())
        } else {
            CalDateTime::Floating(date)
        };
        Ok(CalDate::DateTime(res))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_str as from_json_str, to_string as to_json_string};

    #[test]
    fn simple_date() {
        let prop = "DUE:19990506".parse::<Property>().unwrap();
        let date: CalDate = prop.try_into().unwrap();
        assert_eq!(
            date,
            CalDate::Date(
                NaiveDate::from_ymd_opt(1999, 5, 6).unwrap(),
                CalDateType::Inclusive
            )
        );
    }

    #[test]
    fn date_with_value() {
        let prop = "DUE;VALUE=DATE:20041030".parse::<Property>().unwrap();
        let date: CalDate = prop.try_into().unwrap();
        assert_eq!(
            date,
            CalDate::Date(
                NaiveDate::from_ymd_opt(2004, 10, 30).unwrap(),
                CalDateType::Inclusive
            )
        );
    }

    #[test]
    fn datetime_tz() {
        let prop = "DTSTART;TZID=Europe/Berlin:20040102T081000"
            .parse::<Property>()
            .unwrap();
        let date: CalDate = prop.try_into().unwrap();

        let expected = NaiveDate::from_ymd_opt(2004, 1, 2)
            .and_then(|d| d.and_hms_opt(8, 10, 0))
            .unwrap();
        assert_eq!(
            date,
            CalDate::DateTime(CalDateTime::Timezone(expected, "Europe/Berlin".to_string()))
        );
    }

    #[test]
    fn datetime_utc() {
        let prop = "DTSTART:20241231T125622Z".parse::<Property>().unwrap();
        let date: CalDate = prop.try_into().unwrap();

        let expected = NaiveDate::from_ymd_opt(2024, 12, 31)
            .and_then(|d| d.and_hms_opt(12, 56, 22))
            .unwrap();
        assert_eq!(
            date,
            CalDate::DateTime(CalDateTime::Utc(expected.and_utc()))
        );
    }

    #[test]
    fn datetime_floating() {
        let prop = "DTSTART:18900622T002310".parse::<Property>().unwrap();
        let date: CalDate = prop.try_into().unwrap();

        let expected = NaiveDate::from_ymd_opt(1890, 6, 22)
            .and_then(|d| d.and_hms_opt(0, 23, 10))
            .unwrap();
        assert_eq!(date, CalDate::DateTime(CalDateTime::Floating(expected)));
    }

    #[test]
    fn ser_date() {
        let date = CalDate::Date(
            NaiveDate::from_ymd_opt(1967, 10, 22).unwrap(),
            CalDateType::Inclusive,
        );
        let date_str = date.to_string();
        let str_date = date_str.parse().unwrap();
        assert_eq!(date, str_date);
    }

    #[test]
    fn ser_datetime_floating() {
        let date = CalDate::DateTime(CalDateTime::Floating(
            NaiveDate::from_ymd_opt(1967, 10, 22)
                .and_then(|d| d.and_hms_opt(10, 16, 22))
                .unwrap(),
        ));
        let date_str = date.to_string();
        let str_date = date_str.parse().unwrap();
        assert_eq!(date, str_date);
    }

    #[test]
    fn ser_datetime_tz() {
        let date = CalDate::DateTime(CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(1967, 10, 22)
                .and_then(|d| d.and_hms_opt(10, 16, 22))
                .unwrap(),
            "Europe/Berlin".to_string(),
        ));
        let date_str = date.to_string();
        let str_date = date_str.parse().unwrap();
        assert_eq!(date, str_date);
    }

    #[test]
    fn ser_datetime_utc() {
        let date = CalDate::DateTime(CalDateTime::Utc(
            NaiveDate::from_ymd_opt(1967, 10, 22)
                .and_then(|d| d.and_hms_opt(10, 16, 22))
                .unwrap()
                .and_utc(),
        ));
        let date_str = date.to_string();
        let str_date = date_str.parse().unwrap();
        assert_eq!(date, str_date);
    }

    #[test]
    fn date_type_parsing_and_component_mapping() {
        assert_eq!(
            "Exclusive".parse::<CalDateType>().unwrap(),
            CalDateType::Exclusive
        );
        assert_eq!(CalDateType::from(CalCompType::Todo), CalDateType::Inclusive);
        assert_eq!(CalDateType::Inclusive.to_string(), "Inclusive");

        let err = "NotADateType".parse::<CalDateType>().unwrap_err();
        assert_eq!(err, ParseError::InvalidDate("NotADateType".to_string()));
    }

    #[test]
    fn date_serde_and_inclusive_end_formatting() {
        let date = CalDate::new_date(
            NaiveDate::from_ymd_opt(2024, 2, 3).unwrap(),
            CalDateType::Inclusive,
        );
        let ctx = DateContext::system();

        assert_eq!(ctx.date(&date).fmt_start_in(&Tz::UTC), "February 03, 2024");
        assert_eq!(ctx.date(&date).fmt_end_in(&Tz::UTC), "February 03, 2024");
        assert_eq!(
            ctx.date(&date).end_in(&Tz::UTC).to_rfc3339(),
            "2024-02-03T23:59:59+00:00"
        );

        let json = to_json_string(&date).unwrap();
        assert_eq!(json, "\"D2024-02-03;Inclusive\"");
        let from_json: CalDate = from_json_str(&json).unwrap();
        assert_eq!(from_json, date);
    }

    #[test]
    fn caldate_conversions_to_and_from_utc() {
        let berlin = Tz::Europe__Berlin
            .with_ymd_and_hms(2024, 1, 2, 3, 4, 5)
            .single()
            .unwrap();
        let from_datetime = CalDate::from(berlin);
        let berlin_ctx = DateContext::system();
        assert_eq!(
            from_datetime.to_string(),
            "TTEurope/Berlin;2024-01-02T03:04:05"
        );
        assert_eq!(
            berlin_ctx
                .date_to_utc(&from_datetime, &Tz::Europe__Berlin)
                .to_string(),
            "TU2024-01-02T02:04:05"
        );

        let plain_date = CalDate::Date(
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            CalDateType::Exclusive,
        );
        assert_eq!(
            berlin_ctx.date_to_utc(&plain_date, &Tz::Europe__Berlin),
            plain_date
        );
    }

    #[test]
    fn caldate_equality_and_ordering_are_structural() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let utc = CalDate::DateTime(CalDateTime::Utc(
            NaiveDate::from_ymd_opt(2024, 1, 2)
                .and_then(|d| d.and_hms_opt(8, 0, 0))
                .unwrap()
                .and_utc(),
        ));
        let berlin = CalDate::DateTime(CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(2024, 1, 2)
                .and_then(|d| d.and_hms_opt(9, 0, 0))
                .unwrap(),
            "Europe/Berlin".to_string(),
        ));

        let mut utc_hash = DefaultHasher::new();
        utc.hash(&mut utc_hash);
        let mut berlin_hash = DefaultHasher::new();
        berlin.hash(&mut berlin_hash);

        assert_ne!(utc, berlin);
        assert_ne!(utc_hash.finish(), berlin_hash.finish());
        assert!(utc < berlin || berlin < utc);
    }

    #[test]
    fn bound_dates_compare_by_resolved_instant() {
        let utc = CalDate::DateTime(CalDateTime::Utc(
            NaiveDate::from_ymd_opt(2024, 1, 2)
                .and_then(|d| d.and_hms_opt(8, 0, 0))
                .unwrap()
                .and_utc(),
        ));
        let berlin = CalDate::DateTime(CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(2024, 1, 2)
                .and_then(|d| d.and_hms_opt(9, 0, 0))
                .unwrap(),
            "Europe/Berlin".to_string(),
        ));
        let ctx = DateContext::system();

        assert_eq!(
            ctx.date(&utc).resolved_start(&Tz::UTC),
            ctx.date(&berlin).resolved_start(&Tz::UTC)
        );
    }

    #[test]
    fn caldate_from_str_errors_are_specific() {
        assert_eq!(
            "X2024-01-02".parse::<CalDate>().unwrap_err(),
            ParseError::MalformedDate("X2024-01-02".to_string())
        );
        assert_eq!(
            "Dnot-a-date;Inclusive".parse::<CalDate>().unwrap_err(),
            ParseError::MalformedDate("Dnot-a-date;Inclusive".to_string())
        );
        assert_eq!(
            "D2024-01-02;Unknown".parse::<CalDate>().unwrap_err(),
            ParseError::MalformedDate("D2024-01-02;Unknown".to_string())
        );
        assert_eq!(
            "Tnot-a-datetime".parse::<CalDate>().unwrap_err(),
            ParseError::MalformedDate("Tnot-a-datetime".to_string())
        );
    }

    #[test]
    fn caldatetime_helpers_cover_each_variant() {
        let naive = NaiveDate::from_ymd_opt(2024, 1, 15)
            .and_then(|d| d.and_hms_opt(8, 9, 10))
            .unwrap();

        let utc = CalDateTime::Utc(naive.and_utc());
        let floating = CalDateTime::Floating(naive);
        let timezone = CalDateTime::Timezone(naive, "Europe/Berlin".to_string());

        assert_eq!(timezone.as_naive_date().to_string(), "2024-01-15");
        assert_eq!(floating.as_naive_date().to_string(), "2024-01-15");

        assert_eq!(utc.as_naive_time().to_string(), "08:09:10");
        assert_eq!(timezone.as_naive_time().to_string(), "08:09:10");
        assert_eq!(floating.as_naive_time().to_string(), "08:09:10");

        let berlin_ctx = DateContext::system();
        let utc_ctx = DateContext::system();

        assert_eq!(
            berlin_ctx
                .resolve_datetime(&utc, &Tz::Europe__Berlin)
                .with_timezone(&Tz::UTC)
                .to_rfc3339(),
            "2024-01-15T08:09:10+00:00"
        );

        let fallback_timezone = CalDateTime::Timezone(naive, "Mars/Phobos".to_string());
        assert_eq!(
            utc_ctx
                .resolve_datetime(&fallback_timezone, &Tz::UTC)
                .with_timezone(&Tz::UTC)
                .to_rfc3339(),
            "2024-01-15T08:09:10+00:00"
        );
    }

    #[test]
    fn caldatetime_to_prop_to_utc_and_parse_errors() {
        let floating = CalDateTime::Floating(
            NaiveDate::from_ymd_opt(2024, 2, 3)
                .and_then(|d| d.and_hms_opt(4, 5, 6))
                .unwrap(),
        );

        assert_eq!(
            floating.to_prop("DTSTART"),
            Property::new("DTSTART", vec![], "20240203T040506")
        );
        assert_eq!(
            DateContext::system()
                .datetime_to_utc(&floating, &Tz::UTC)
                .to_string(),
            "U2024-02-03T04:05:06"
        );

        assert_eq!(
            "X2024-02-03T04:05:06".parse::<CalDateTime>().unwrap_err(),
            ParseError::MalformedDate("X2024-02-03T04:05:06".to_string())
        );
        assert_eq!(
            "Fbad".parse::<CalDateTime>().unwrap_err(),
            ParseError::MalformedDate("fooFbad".to_string())
        );
        assert_eq!(
            "Ubad".parse::<CalDateTime>().unwrap_err(),
            ParseError::MalformedDate("Ubad".to_string())
        );
        assert_eq!(
            "TEurope/Berlin;bad".parse::<CalDateTime>().unwrap_err(),
            ParseError::MalformedDate("TEurope/Berlin;bad".to_string())
        );
    }

    #[test]
    fn caldate_try_from_property_covers_exclusive_and_errors() {
        let no_time: CalDate = "DTSTART:20240203"
            .parse::<Property>()
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(
            no_time,
            CalDate::Date(
                NaiveDate::from_ymd_opt(2024, 2, 3).unwrap(),
                CalDateType::Exclusive
            )
        );

        let value_date: CalDate = "DTSTART;VALUE=DATE:20240204"
            .parse::<Property>()
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(
            value_date,
            CalDate::Date(
                NaiveDate::from_ymd_opt(2024, 2, 4).unwrap(),
                CalDateType::Exclusive
            )
        );

        let short_err = CalDate::try_from(Property::new("DTSTART", vec![], "2024")).unwrap_err();
        assert_eq!(short_err, ParseError::MalformedDate("2024".to_string()));

        let malformed_err =
            CalDate::try_from("DTSTART:20240203X040506".parse::<Property>().unwrap()).unwrap_err();
        assert_eq!(
            malformed_err,
            ParseError::MalformedDate("20240203X040506".to_string())
        );

        let invalid_date_err =
            CalDate::try_from("DTSTART:20240230T040506".parse::<Property>().unwrap()).unwrap_err();
        assert_eq!(
            invalid_date_err,
            ParseError::InvalidDate("20240230T040506".to_string())
        );

        let invalid_due_date_err =
            CalDate::try_from("DUE;VALUE=DATE:20240230".parse::<Property>().unwrap()).unwrap_err();
        assert_eq!(
            invalid_due_date_err,
            ParseError::InvalidDate("20240230".to_string())
        );

        let invalid_number_err =
            CalDate::try_from("DTSTART:abcd0203T040506".parse::<Property>().unwrap()).unwrap_err();
        assert!(matches!(invalid_number_err, ParseError::InvalidNumber(_)));
    }

    // --- DST gap and fold validation ---

    #[test]
    fn validate_utc_always_passes() {
        let dt = CalDateTime::Utc(
            NaiveDate::from_ymd_opt(2025, 3, 30)
                .and_then(|d| d.and_hms_opt(2, 30, 0))
                .unwrap()
                .and_utc(),
        );
        // 2:30 AM doesn't exist in Europe/Berlin on 2025-03-30, but UTC is always valid.
        assert!(
            DateContext::system()
                .validate_datetime(&dt, &Tz::Europe__Berlin)
                .is_ok()
        );
    }

    #[test]
    fn validate_floating_rejects_dst_gap() {
        let dt = CalDateTime::Floating(
            NaiveDate::from_ymd_opt(2025, 3, 30)
                .and_then(|d| d.and_hms_opt(2, 30, 0))
                .unwrap(),
        );
        // 2:30 AM doesn't exist in Europe/Berlin on 2025-03-30 (spring forward).
        assert!(
            DateContext::system()
                .validate_datetime(&dt, &Tz::Europe__Berlin)
                .is_err()
        );
    }

    #[test]
    fn validate_floating_accepts_valid_time() {
        let dt = CalDateTime::Floating(
            NaiveDate::from_ymd_opt(2025, 3, 30)
                .and_then(|d| d.and_hms_opt(10, 0, 0))
                .unwrap(),
        );
        assert!(
            DateContext::system()
                .validate_datetime(&dt, &Tz::Europe__Berlin)
                .is_ok()
        );
    }

    #[test]
    fn validate_timezone_rejects_declared_tz_gap() {
        // 2:30 AM doesn't exist in Europe/Berlin on 2025-03-30.
        let dt = CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(2025, 3, 30)
                .and_then(|d| d.and_hms_opt(2, 30, 0))
                .unwrap(),
            "Europe/Berlin".to_string(),
        );
        // Even if local_tz is UTC (where it's fine), the declared tz rejects it.
        assert!(
            DateContext::system()
                .validate_datetime(&dt, &Tz::UTC)
                .is_err()
        );
    }

    #[test]
    fn validate_timezone_rejects_local_tz_gap() {
        // 2:00 AM doesn't exist in America/New_York on 2025-03-09 (spring forward).
        // Use a time that's valid in Europe/Berlin but not in America/New_York.
        let dt = CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(2025, 3, 9)
                .and_then(|d| d.and_hms_opt(2, 30, 0))
                .unwrap(),
            "Europe/Berlin".to_string(),
        );
        assert!(
            DateContext::system()
                .validate_datetime(&dt, &Tz::America__New_York)
                .is_err()
        );
    }

    #[test]
    fn validate_timezone_accepts_valid_time() {
        let dt = CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(2025, 1, 15)
                .and_then(|d| d.and_hms_opt(10, 0, 0))
                .unwrap(),
            "Europe/Berlin".to_string(),
        );
        assert!(
            DateContext::system()
                .validate_datetime(&dt, &Tz::Europe__Berlin)
                .is_ok()
        );
    }

    #[test]
    fn validate_floating_rejects_dst_fold() {
        let dt = CalDateTime::Floating(
            NaiveDate::from_ymd_opt(2025, 10, 26)
                .and_then(|d| d.and_hms_opt(2, 30, 0))
                .unwrap(),
        );
        // 2:30 AM on 2025-10-26 is ambiguous in Europe/Berlin (fall back).
        assert!(
            DateContext::system()
                .validate_datetime(&dt, &Tz::Europe__Berlin)
                .is_err()
        );
    }

    #[test]
    fn validate_timezone_rejects_declared_tz_fold() {
        // 2:30 AM on 2025-10-26 is ambiguous in Europe/Berlin (fall back).
        let dt = CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(2025, 10, 26)
                .and_then(|d| d.and_hms_opt(2, 30, 0))
                .unwrap(),
            "Europe/Berlin".to_string(),
        );
        // Even if local_tz is UTC (where it's unambiguous), the declared tz rejects it.
        assert!(
            DateContext::system()
                .validate_datetime(&dt, &Tz::UTC)
                .is_err()
        );
    }

    #[test]
    fn validate_timezone_rejects_local_tz_fold() {
        // 1:30 AM on 2025-11-02 is ambiguous in America/New_York (fall back).
        // Use a naive time that is valid in Europe/Berlin but ambiguous in New York.
        let dt = CalDateTime::Timezone(
            NaiveDate::from_ymd_opt(2025, 11, 2)
                .and_then(|d| d.and_hms_opt(1, 30, 0))
                .unwrap(),
            "Europe/Berlin".to_string(),
        );
        assert!(
            DateContext::system()
                .validate_datetime(&dt, &Tz::America__New_York)
                .is_err()
        );
    }

    #[test]
    fn validate_caldate_date_rejects_midnight_gap() {
        // Cuba (America/Havana) had midnight DST transitions historically
        // (e.g., 2007-03-11 00:00 sprang forward to 01:00). Use that.
        let date = CalDate::Date(
            NaiveDate::from_ymd_opt(2007, 3, 11).unwrap(),
            CalDateType::Inclusive,
        );
        let result = DateContext::system().validate_date(&date, &Tz::America__Havana);
        // Midnight doesn't exist on this date in Havana.
        assert!(result.is_err());
    }

    #[test]
    fn validate_caldate_date_accepts_normal_date() {
        let date = CalDate::Date(
            NaiveDate::from_ymd_opt(2025, 6, 15).unwrap(),
            CalDateType::Exclusive,
        );
        assert!(
            DateContext::system()
                .validate_date(&date, &Tz::Europe__Berlin)
                .is_ok()
        );
    }
}
