use std::{cmp::Ordering, fmt, hash::Hash, str::FromStr};

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::{Europe, Tz};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::parser::{Parameter, ParseError, Property};

use super::CalCompType;

/// The type of date.
///
/// The iCalendar format has interestingly two different ways to interpret dates of type
/// [`CalDate::Date`]. For events, the end is interpreted as "exclusive", meaning that an event
/// that starts on 2025-02-23 and ends on 2025-02-24 is actually just one day long (the entire
/// 2025-02-23) and ends at the start of 2025-02-24. For TODOs however, the due date is
/// "inclusive". For example, if the due date is 2025-02-23, the TODO is due until the *end* of
/// that day.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
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
#[derive(Debug, Clone)]
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

    /// Returns a string representation when using this date as the start of an event.
    ///
    /// Note that start/end makes a difference when this date is of variant [`CalDate::Date`],
    /// because its interpretation of the end is different depending on the context. See
    /// [`Self::as_end_with_tz`] for a detailed description.
    pub fn fmt_start_with_tz(&self, tz: &Tz) -> String {
        self.fmt_date(self.as_start_with_tz(tz))
    }

    /// Returns a string representation when using this date as the end of an event.
    ///
    /// Note that start/end makes a difference when this date is of variant [`CalDate::Date`],
    /// because its interpretation of the end is different depending on the context. See
    /// [`Self::as_end_with_tz`] for a detailed description.
    pub fn fmt_end_with_tz(&self, tz: &Tz) -> String {
        self.fmt_date(self.as_end_with_tz(tz))
    }

    fn fmt_date(&self, dt: DateTime<Tz>) -> String {
        match self {
            Self::Date(..) => dt.format("%B %d, %Y").to_string(),
            Self::DateTime(_) => dt.format("%A, %B %d, %Y %H:%M").to_string(),
        }
    }

    /// Returns the [`NaiveDate`] instance corresponding to this [`CalDate`].
    pub fn as_naive_date(&self) -> NaiveDate {
        match self {
            Self::Date(date, _) => *date,
            Self::DateTime(datetime) => datetime.as_naive_date(),
        }
    }

    /// Returns a [`DateTime`] instance for this [`CalDate`].
    ///
    /// If this calendar date has specified a timezone (or is in UTC), this timezone will be used.
    /// If this calendar date is floating, the given timezone will be used. Furthermore, if this
    /// calendar date is not a [`CalDate::DateTime`], the given timezone will be used.
    pub fn as_datetime(&self, local: &Tz) -> DateTime<Tz> {
        match self {
            Self::Date(date, _ty) => local
                .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                .unwrap(),
            Self::DateTime(datetime) => datetime.as_datetime(local),
        }
    }

    /// Returns a [`CalDate`] instance with UTC time.
    pub fn to_utc(self) -> CalDate {
        match self {
            Self::Date(date, ty) => Self::Date(date, ty),
            Self::DateTime(datetime) => Self::DateTime(datetime.to_utc()),
        }
    }

    /// Returns the corresponding [`DateTime`] instance when using this date as the start of an
    /// event.
    ///
    /// Note that start/end makes a difference when this date is of variant [`CalDate::Date`],
    /// because its interpretation of the end is different depending on the context. See
    /// [`Self::as_end_with_tz`] for a detailed description.
    pub fn as_start_with_tz(&self, tz: &Tz) -> DateTime<Tz> {
        match self {
            Self::Date(date, _) => tz
                .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                .unwrap(),
            Self::DateTime(datetime) => datetime.with_tz(tz),
        }
    }

    /// Returns the corresponding [`DateTime`] instance when using this date as the end of an
    /// event.
    ///
    /// In contrast to the iCalendar format, which interpretes dates sometimes inclusive and
    /// sometimes exclusive, we define the end of an event (or the due date of a TODO) always in an
    /// inclusive sense. That is, an event that starts on 2025-02-23 and and is one day long ends
    /// on 2025-02-23.
    ///
    /// As this method requests the [`DateTime`] for this date as the end of an event, this method
    /// returns the end of the last day. In the above example that would be 2025-02-23 23:59:59.
    /// Conversely, [`Self::as_start_with_tz`] would return 2025-02-23 00:00:00.
    pub fn as_end_with_tz(&self, tz: &Tz) -> DateTime<Tz> {
        match self {
            Self::Date(date, CalDateType::Exclusive) => {
                let next_day = tz
                    .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                    .unwrap();
                next_day - Duration::seconds(1)
            }
            Self::Date(date, CalDateType::Inclusive) => tz
                .from_local_datetime(&date.and_hms_opt(23, 59, 59).unwrap())
                .unwrap(),
            Self::DateTime(datetime) => datetime.with_tz(tz),
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
}

impl From<DateTime<Tz>> for CalDate {
    fn from(date: DateTime<Tz>) -> Self {
        Self::DateTime(CalDateTime::Timezone(
            date.naive_local(),
            date.timezone().name().to_string(),
        ))
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

impl Hash for CalDate {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_start_with_tz(&chrono_tz::UTC).hash(state);
    }
}

impl PartialEq for CalDate {
    fn eq(&self, other: &Self) -> bool {
        let a = self.as_start_with_tz(&chrono_tz::UTC);
        let b = other.as_start_with_tz(&chrono_tz::UTC);
        a == b
    }
}

impl Eq for CalDate {}

impl PartialOrd for CalDate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CalDate {
    fn cmp(&self, other: &Self) -> Ordering {
        let a = self.as_start_with_tz(&chrono_tz::UTC);
        let b = other.as_start_with_tz(&chrono_tz::UTC);
        a.cmp(&b)
    }
}

/// An iCalendar date in datetime format.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
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

    /// Returns the corresponding [`DateTime`] instance with the given timezone.
    pub fn with_tz(&self, tz: &Tz) -> DateTime<Tz> {
        match self {
            Self::Utc(dt) => dt.with_timezone(tz),
            Self::Timezone(dt, tzid) => Self::resolve_timezone(*dt, tzid).with_timezone(tz),
            Self::Floating(dt) => tz.from_local_datetime(dt).unwrap(),
        }
    }

    /// Returns a [`DateTime`] instance for this [`CalDate`].
    ///
    /// If this calendar date has specified a timezone (or is in UTC), this timezone will be used.
    /// If this calendar date is floating, the given timezone will be used.
    pub fn as_datetime(&self, local: &Tz) -> DateTime<Tz> {
        match self {
            Self::Utc(dt) => dt.with_timezone(&Tz::UTC),
            Self::Timezone(dt, tzid) => Self::resolve_timezone(*dt, tzid),
            Self::Floating(dt) => local.from_local_datetime(dt).unwrap(),
        }
    }

    fn resolve_timezone(dt: NaiveDateTime, tzid: &str) -> DateTime<Tz> {
        let date_tz = if let Ok(date_tz) = tzid.parse::<Tz>() {
            date_tz
        } else {
            // we fall back to Europe/Berlin for all weird values that we see
            // TODO this is temporary
            Europe::Berlin
        };
        date_tz.from_local_datetime(&dt).unwrap()
    }

    /// Returns a [`CalDateTime`] instance with UTC time.
    pub fn to_utc(self) -> CalDateTime {
        let dt = self.with_tz(&Tz::UTC);
        Self::Utc(dt.to_utc())
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
}
