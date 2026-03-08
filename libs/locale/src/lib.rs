//! Locale-aware date and time formatting for calendar events.
//!
//! This crate provides the [`Locale`] trait and concrete implementations for English
//! ([`LocaleEn`]) and German ([`LocaleDe`]). A locale combines a timezone with an optional
//! translations table loaded from a TOML file, and exposes methods for formatting dates, times,
//! and date ranges in a human-readable, language-appropriate form.
//!
//! # Quick start
//!
//! The simplest way to obtain a locale is [`default`], which returns an English locale with the
//! UTC timezone and no translations:
//!
//! ```
//! let locale = eventix_locale::default();
//! ```
//!
//! To create a locale that reads translations from an XDG data directory and detects the system
//! timezone, use [`new`]:
//!
//! ```no_run
//! use eventix_locale::{self, LocaleType};
//! use xdg::BaseDirectories;
//!
//! let xdg = BaseDirectories::new();
//! let locale = eventix_locale::new(&xdg, LocaleType::German).unwrap();
//! ```
//!
//! # Formatting
//!
//! Once you have a locale, use [`Locale::fmt_date`], [`Locale::fmt_time`],
//! [`Locale::fmt_datetime`], or [`Locale::date_range`] to produce display strings. Pass
//! [`DateFlags`] or [`TimeFlags`] to control short vs. long output and to suppress the
//! automatic substitution of relative labels ("Today", "Tomorrow", "Yesterday").
//!
//! # Translations
//!
//! Translation tables are plain TOML files with a `[table]` section mapping English keys to
//! their locale-specific equivalents. Keys used by the formatting methods include weekday names
//! (`"Monday"`, …), abbreviated weekday names (`"Mon"`, …), month names (`"January"`, …),
//! abbreviated month names (`"Jan"`, …), and the relative labels `"Today"`, `"Tomorrow"`, and
//! `"Yesterday"`. Any key absent from the table is returned unchanged, so an empty table
//! produces English output.

mod de;
mod en;

use bitflags::bitflags;
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use eventix_ical::objects::{CalDate, CalLocale};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Debug,
    fs::OpenOptions,
    io::{self, Read},
    path::Path,
    sync::Arc,
};
use thiserror::Error;
use xdg::BaseDirectories;

pub use de::LocaleDe;
#[allow(unused_imports)]
pub use en::LocaleEn;

/// Errors that can occur when constructing a locale.
#[derive(Debug, Error)]
pub enum LocaleError {
    #[error("Locale file '$XDG_DATA_HOME/{0}' not found: {0}")]
    LocaleFile(String),
    #[error("System timezone not found")]
    SysTimezone,
    #[error("Parsing system timezone '{0}' failed")]
    ParseTimezone(String),
    #[error("Reading locale file failed: {0}")]
    ReadLocale(io::Error),
}

/// Identifies the language used by a [`Locale`] implementation.
#[derive(Copy, Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum LocaleType {
    German,
    #[default]
    English,
}

/// A flat key-to-value mapping loaded from a TOML translations file.
///
/// The `table` field maps English display strings (weekday names, month names, and relative
/// labels such as `"Today"`) to their locale-specific equivalents. Keys absent from the table
/// are passed through unchanged, so an empty table produces English output.
#[derive(Default, Debug, Deserialize)]
pub struct Translations {
    pub table: HashMap<String, String>,
}

impl Translations {
    /// Reads and parses a translations table from a TOML file at `path`.
    ///
    /// The file must contain a `[table]` section with string key-value pairs. Returns an
    /// `io::Error` if the file cannot be opened or read.
    pub(crate) fn new_from_file(path: &Path) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).open(path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        Ok(toml::from_str(&content).unwrap())
    }
}

/// A type that exposes a calendar date as both a [`NaiveDate`] and a strftime-formatted string.
///
/// This trait is implemented for [`DateTime<Tz>`], [`NaiveDate`], and their shared references,
/// allowing formatting methods on [`Locale`] to accept any of these types uniformly.
pub trait DateLike {
    /// Returns the date portion as a [`NaiveDate`], discarding any time or timezone information.
    fn naive(&self) -> NaiveDate;

    /// Formats the date (and time, if present) using the given strftime format string.
    fn fmt(&self, f: &str) -> String;
}

impl DateLike for DateTime<Tz> {
    fn naive(&self) -> NaiveDate {
        self.date_naive()
    }

    fn fmt(&self, f: &str) -> String {
        self.format(f).to_string()
    }
}

impl DateLike for &DateTime<Tz> {
    fn naive(&self) -> NaiveDate {
        self.date_naive()
    }

    fn fmt(&self, f: &str) -> String {
        self.format(f).to_string()
    }
}

impl DateLike for NaiveDate {
    fn naive(&self) -> NaiveDate {
        *self
    }

    fn fmt(&self, f: &str) -> String {
        self.format(f).to_string()
    }
}

impl DateLike for &NaiveDate {
    fn naive(&self) -> NaiveDate {
        **self
    }

    fn fmt(&self, f: &str) -> String {
        self.format(f).to_string()
    }
}

bitflags! {
    /// Controls how a time value is rendered by [`Locale::fmt_time`].
    #[derive(Copy, Clone)]
    pub struct TimeFlags : u32 {
        /// No special formatting; render the full time including seconds (`HH:MM:SS`).
        const None = 0;
        /// Short time format: omit seconds and render `HH:MM`.
        const Short = 1;
    }
}

bitflags! {
    /// Controls how a date value is rendered by [`Locale::fmt_date`] and related methods.
    #[derive(Copy, Clone)]
    pub struct DateFlags: u32 {
        /// No special formatting; use the long, verbose date form (includes weekday and full
        /// month name).
        const None = 0;
        /// Short date form (abbreviated month name and no weekday), e.g. `Jun 15, 2024`.
        const Short = 1;
        /// Suppress automatic substitution of relative labels ("Today", "Tomorrow",
        /// "Yesterday"). When set, formatting always emits a concrete date.
        const NoToday = 2;
    }
}

impl From<DateFlags> for TimeFlags {
    fn from(flags: DateFlags) -> Self {
        if flags.contains(DateFlags::Short) {
            TimeFlags::Short
        } else {
            TimeFlags::empty()
        }
    }
}

/// Language- and timezone-aware formatting of dates, times, and date ranges.
///
/// A `Locale` combines the translation and timezone capabilities of [`CalLocale`] with
/// higher-level formatting methods. The default implementations produce English output;
/// locale-specific implementations (e.g. [`LocaleDe`]) override the methods that require
/// translated weekday or month names.
pub trait Locale: CalLocale + Debug {
    /// Returns the language variant of this locale.
    fn ty(&self) -> LocaleType;

    /// Returns the translations table used to look up display strings.
    fn translations(&self) -> &Translations;

    /// Formats a time as `HH:MM` (short) or `HH:MM:SS` (long).
    fn fmt_time(&self, date: &dyn DateLike, flags: TimeFlags) -> String {
        let fmt = if flags.contains(TimeFlags::Short) {
            "%H:%M"
        } else {
            "%H:%M:%S"
        };
        date.fmt(fmt)
    }

    /// Formats a date with the day-of-week prepended: e.g. `Sat, Jun 15` (short) or
    /// `Saturday, Jun 15 2024` (long).
    ///
    /// Substitutes a relative label ("Today", "Tomorrow", "Yesterday") when the date falls
    /// within one day of today, unless [`DateFlags::NoToday`] is set.
    fn fmt_weekdate(&self, date: &dyn DateLike, flags: DateFlags) -> String {
        let fmt = if flags.contains(DateFlags::Short) {
            "%a, %b %d"
        } else {
            "%A, %b %d %Y"
        };
        self.fmt_date_with(date, fmt, flags)
    }

    /// Formats a date: e.g. `Jun 15, 2024` (short) or `Saturday, June 15, 2024` (long).
    ///
    /// Substitutes a relative label ("Today", "Tomorrow", "Yesterday") when the date falls
    /// within one day of today, unless [`DateFlags::NoToday`] is set.
    fn fmt_date(&self, date: &dyn DateLike, flags: DateFlags) -> String {
        let fmt = if flags.contains(DateFlags::Short) {
            "%b %d, %Y"
        } else {
            "%A, %B %d, %Y"
        };
        self.fmt_date_with(date, fmt, flags)
    }

    /// Formats a date and time together, separated by a comma: e.g. `Jun 15, 2024, 09:05`.
    fn fmt_datetime(&self, date: &dyn DateLike, flags: DateFlags) -> String {
        format!(
            "{}, {}",
            self.fmt_date(date, flags),
            self.fmt_time(date, flags.into())
        )
    }

    /// Formats a date using an explicit strftime format string `f`.
    ///
    /// If `flags` does not contain [`DateFlags::NoToday`] and the date is today, tomorrow, or
    /// yesterday, the relative label is returned instead of applying `f`.
    fn fmt_date_with(&self, date: &dyn DateLike, f: &str, flags: DateFlags) -> String {
        if !flags.contains(DateFlags::NoToday)
            && let Some(rel) = self.has_relative(date)
        {
            return rel.to_string();
        }
        date.fmt(f).to_string()
    }

    /// Returns a translated relative label if `date` is today, tomorrow, or yesterday.
    ///
    /// Returns `Some("Today")`, `Some("Tomorrow")`, or `Some("Yesterday")` (subject to
    /// translation), or `None` if the date does not fall within that window.
    fn has_relative(&self, date: &dyn DateLike) -> Option<&str> {
        let today = Utc::now().date_naive();
        if date.naive() == today {
            return Some(self.translate("Today"));
        } else if Some(date.naive()) == today.succ_opt() {
            return Some(self.translate("Tomorrow"));
        } else if Some(date.naive()) == today.pred_opt() {
            return Some(self.translate("Yesterday"));
        }
        None
    }

    /// Formats a start/end date pair as a human-readable range string.
    ///
    /// The output depends on the combination of start and end values:
    ///
    /// - Both absent → `"-"`.
    /// - Date-only start with no end, or date-only end with no start → the single date.
    /// - Date-only start + date-only end where end is the day immediately after start →
    ///   the single start date (all-day event spanning exactly one day).
    /// - Date-only start + date-only end spanning multiple days →
    ///   `"<start> &#x2012; <end>"`.
    /// - DateTime start and end on the same calendar day →
    ///   `"<date>, <start-time> &#x2012; <end-time>"`.
    /// - DateTime start and end on different days →
    ///   `"<start-datetime> &#x2012; <end-datetime>"`.
    /// - DateTime-only start or end → formatted with [`Locale::fmt_datetime`].
    ///
    /// All dates are formatted in the short style using the locale's timezone.
    fn date_range(&self, start: Option<CalDate>, end: Option<CalDate>) -> String {
        let tz = self.timezone();
        let date_flags = DateFlags::Short;
        let time_flags = TimeFlags::Short;
        match (start, end) {
            (Some(CalDate::Date(start, ..)), Some(CalDate::Date(end, ..)))
                if start.succ_opt() == Some(end) =>
            {
                self.fmt_date(&start, date_flags).to_string()
            }
            (Some(CalDate::Date(start, ..)), Some(end @ CalDate::Date(..))) => {
                format!(
                    "{} &#x2012; {}",
                    self.fmt_date(&start, date_flags),
                    self.fmt_date(&end.as_end_with_tz(tz), date_flags)
                )
            }
            (Some(start), Some(end)) if start.as_naive_date() == end.as_naive_date() => {
                format!(
                    "{}, {} &#x2012; {}",
                    self.fmt_date(&start.as_naive_date(), date_flags),
                    self.fmt_time(&start.as_start_with_tz(tz), time_flags),
                    self.fmt_time(&end.as_end_with_tz(tz), time_flags)
                )
            }
            (Some(start), Some(end)) => {
                format!(
                    "{} &#x2012; {}",
                    self.fmt_datetime(&start.as_start_with_tz(tz), date_flags),
                    self.fmt_datetime(&end.as_end_with_tz(tz), date_flags)
                )
            }
            (Some(CalDate::Date(start, ..)), None) => self.fmt_date(&start, date_flags),
            (Some(start @ CalDate::DateTime(_)), None) => {
                self.fmt_datetime(&start.as_start_with_tz(tz), date_flags)
            }
            (None, Some(CalDate::Date(end, ..))) => self.fmt_date(&end, date_flags),
            (None, Some(end @ CalDate::DateTime(_))) => {
                self.fmt_datetime(&end.as_end_with_tz(tz), date_flags)
            }
            (None, None) => String::from("-"),
        }
    }
}

/// Returns a default English locale with the UTC timezone and no translations.
pub fn default() -> Arc<dyn Locale + Send + Sync> {
    Arc::new(LocaleEn::default())
}

/// Creates a locale of the given `lang` type, loading translations from the XDG data directory
/// and detecting the system timezone.
///
/// The translations file is looked up as `locale/<Lang>.toml` under `$XDG_DATA_HOME` (and the
/// standard XDG data dirs). Returns a [`LocaleError`] if the file is not found, the system
/// timezone cannot be determined, or the timezone string cannot be parsed.
pub fn new(
    xdg: &BaseDirectories,
    lang: LocaleType,
) -> Result<Arc<dyn Locale + Send + Sync>, LocaleError> {
    let trans_file = format!("locale/{:?}.toml", lang);
    let translations = xdg
        .find_data_file(&trans_file)
        .ok_or(LocaleError::LocaleFile(trans_file))?;

    let tz_str = iana_time_zone::get_timezone().map_err(|_| LocaleError::SysTimezone)?;
    let tz: chrono_tz::Tz = tz_str
        .parse()
        .map_err(|_| LocaleError::ParseTimezone(tz_str))?;

    Ok(match lang {
        LocaleType::German => {
            Arc::new(LocaleDe::new(tz, &translations).map_err(LocaleError::ReadLocale)?)
        }
        LocaleType::English => {
            Arc::new(LocaleEn::new(tz, &translations).map_err(LocaleError::ReadLocale)?)
        }
    })
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
    use chrono_tz::Tz;
    use eventix_ical::objects::{CalDate, CalDateTime, CalDateType};

    use crate::{DateFlags, DateLike, LocaleEn, LocaleType, TimeFlags, Translations};

    use super::Locale;

    // --- helpers ---

    fn make_locale_en() -> LocaleEn {
        LocaleEn::default()
    }

    fn fixed_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 6, 15).unwrap()
    }

    fn fixed_datetime_tz() -> chrono::DateTime<Tz> {
        Tz::UTC.from_utc_datetime(&NaiveDateTime::new(
            fixed_date(),
            NaiveTime::from_hms_opt(9, 5, 3).unwrap(),
        ))
    }

    // --- DateLike: DateTime<Tz> ---

    #[test]
    fn datelike_datetime_tz_naive_and_fmt() {
        let dt = fixed_datetime_tz();

        assert_eq!(dt.naive(), fixed_date());
        assert_eq!(dt.fmt("%Y-%m-%d"), "2024-06-15");
        assert_eq!(dt.fmt("%H:%M:%S"), "09:05:03");

        let r = &dt;
        assert_eq!(r.fmt("%Y-%m-%d"), "2024-06-15");
    }

    // --- DateLike: NaiveDate ---

    #[test]
    fn datelike_naive_date_naive_and_fmt() {
        let d = fixed_date();

        assert_eq!(d.naive(), d);
        assert_eq!(d.fmt("%d.%m.%Y"), "15.06.2024");

        let r = &d;
        assert_eq!(r.fmt("%d.%m.%Y"), "15.06.2024");
    }

    // --- Locale::fmt_time ---

    #[test]
    fn fmt_time_short() {
        let locale = make_locale_en();
        let dt = fixed_datetime_tz();
        let result = locale.fmt_time(&dt, TimeFlags::Short);
        assert_eq!(result, "09:05");
    }

    #[test]
    fn fmt_time_long() {
        let locale = make_locale_en();
        let dt = fixed_datetime_tz();
        let result = locale.fmt_time(&dt, TimeFlags::None);
        assert_eq!(result, "09:05:03");
    }

    // --- Locale::fmt_date ---

    #[test]
    fn fmt_date_short_no_today() {
        let locale = make_locale_en();
        let d = fixed_date();
        let result = locale.fmt_date(&d, DateFlags::Short | DateFlags::NoToday);
        assert_eq!(result, "Jun 15, 2024");
    }

    #[test]
    fn fmt_date_long_no_today() {
        let locale = make_locale_en();
        let d = fixed_date();
        let result = locale.fmt_date(&d, DateFlags::NoToday);
        assert_eq!(result, "Saturday, June 15, 2024");
    }

    // --- Locale::fmt_weekdate ---

    #[test]
    fn fmt_weekdate_short_no_today() {
        let locale = make_locale_en();
        let d = fixed_date();
        let result = locale.fmt_weekdate(&d, DateFlags::Short | DateFlags::NoToday);
        assert_eq!(result, "Sat, Jun 15");
    }

    #[test]
    fn fmt_weekdate_long_no_today() {
        let locale = make_locale_en();
        let d = fixed_date();
        let result = locale.fmt_weekdate(&d, DateFlags::NoToday);
        assert_eq!(result, "Saturday, Jun 15 2024");
    }

    // --- Locale::fmt_datetime ---

    #[test]
    fn fmt_datetime_short_no_today() {
        let locale = make_locale_en();
        let dt = fixed_datetime_tz();
        let result = locale.fmt_datetime(&dt, DateFlags::Short | DateFlags::NoToday);
        assert_eq!(result, "Jun 15, 2024, 09:05");
    }

    #[test]
    fn fmt_datetime_long_no_today() {
        let locale = make_locale_en();
        let dt = fixed_datetime_tz();
        let result = locale.fmt_datetime(&dt, DateFlags::NoToday);
        assert_eq!(result, "Saturday, June 15, 2024, 09:05:03");
    }

    // --- Locale::fmt_date_with ---

    #[test]
    fn fmt_date_with_no_today_uses_format() {
        let locale = make_locale_en();
        let d = fixed_date();
        let result = locale.fmt_date_with(&d, "%Y/%m/%d", DateFlags::NoToday);
        assert_eq!(result, "2024/06/15");
    }

    #[test]
    fn fmt_date_with_past_date_no_relative() {
        let locale = make_locale_en();
        // A date far in the past will never be today/tomorrow/yesterday.
        let d = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
        let result = locale.fmt_date_with(&d, "%Y-%m-%d", DateFlags::None);
        assert_eq!(result, "2000-01-01");
    }

    // --- Locale::has_relative ---

    #[test]
    fn has_relative_far_past_is_none() {
        let locale = make_locale_en();
        let d = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
        assert!(locale.has_relative(&d).is_none());
    }

    #[test]
    fn has_relative_today_is_some() {
        let locale = make_locale_en();
        let today = Utc::now().date_naive();
        let result = locale.has_relative(&today);
        assert_eq!(result, Some("Today"));
    }

    #[test]
    fn has_relative_tomorrow_is_some() {
        let locale = make_locale_en();
        let tomorrow = Utc::now().date_naive().succ_opt().unwrap();
        let result = locale.has_relative(&tomorrow);
        assert_eq!(result, Some("Tomorrow"));
    }

    #[test]
    fn has_relative_yesterday_is_some() {
        let locale = make_locale_en();
        let yesterday = Utc::now().date_naive().pred_opt().unwrap();
        let result = locale.has_relative(&yesterday);
        assert_eq!(result, Some("Yesterday"));
    }

    // --- Locale::date_range ---

    #[test]
    fn date_range_none_none() {
        let locale = make_locale_en();
        let result = locale.date_range(None, None);
        assert_eq!(result, "-");
    }

    #[test]
    fn date_range_date_only_start() {
        let locale = make_locale_en();
        let d = CalDate::new_date(fixed_date(), CalDateType::Exclusive);
        let result = locale.date_range(Some(d), None);
        assert_eq!(result, "Jun 15, 2024");
    }

    #[test]
    fn date_range_date_only_end() {
        let locale = make_locale_en();
        let d = CalDate::new_date(fixed_date(), CalDateType::Exclusive);
        let result = locale.date_range(None, Some(d));
        assert_eq!(result, "Jun 15, 2024");
    }

    #[test]
    fn date_range_consecutive_dates_shows_single_date() {
        // When end == start.succ, the range collapses to the single start date.
        let locale = make_locale_en();
        let start = CalDate::new_date(fixed_date(), CalDateType::Exclusive);
        let end = CalDate::new_date(fixed_date().succ_opt().unwrap(), CalDateType::Exclusive);
        let result = locale.date_range(Some(start), Some(end));
        assert_eq!(result, "Jun 15, 2024");
    }

    #[test]
    fn date_range_non_consecutive_dates_shows_range() {
        let locale = make_locale_en();
        let start = CalDate::new_date(fixed_date(), CalDateType::Exclusive);
        // Two days later, so succ check won't collapse it.
        let end = CalDate::new_date(
            NaiveDate::from_ymd_opt(2024, 6, 17).unwrap(),
            CalDateType::Exclusive,
        );
        let result = locale.date_range(Some(start), Some(end));
        // End with Exclusive type: as_end_with_tz returns midnight-1s, i.e. June 16 23:59:59.
        // The formatted date of that is Jun 16.
        assert_eq!(result, "Jun 15, 2024 &#x2012; Jun 16, 2024");
    }

    #[test]
    fn date_range_datetime_start_only() {
        let locale = make_locale_en();
        let naive = NaiveDateTime::new(fixed_date(), NaiveTime::from_hms_opt(10, 0, 0).unwrap());
        let start = CalDate::DateTime(CalDateTime::Utc(naive.and_utc()));
        let result = locale.date_range(Some(start), None);
        assert_eq!(result, "Jun 15, 2024, 10:00");
    }

    #[test]
    fn date_range_datetime_end_only() {
        let locale = make_locale_en();
        let naive = NaiveDateTime::new(fixed_date(), NaiveTime::from_hms_opt(10, 0, 0).unwrap());
        let end = CalDate::DateTime(CalDateTime::Utc(naive.and_utc()));
        let result = locale.date_range(None, Some(end));
        assert_eq!(result, "Jun 15, 2024, 10:00");
    }

    #[test]
    fn date_range_same_day_datetimes_shows_time_range() {
        let locale = make_locale_en();
        let d = fixed_date();
        let start = CalDate::DateTime(CalDateTime::Utc(
            NaiveDateTime::new(d, NaiveTime::from_hms_opt(9, 0, 0).unwrap()).and_utc(),
        ));
        let end = CalDate::DateTime(CalDateTime::Utc(
            NaiveDateTime::new(d, NaiveTime::from_hms_opt(17, 30, 0).unwrap()).and_utc(),
        ));
        let result = locale.date_range(Some(start), Some(end));
        // Expects: "<date>, <start_time> &#x2012; <end_time>"
        assert_eq!(result, "Jun 15, 2024, 09:00 &#x2012; 17:30");
    }

    #[test]
    fn date_range_different_day_datetimes_shows_full_range() {
        let locale = make_locale_en();
        let start = CalDate::DateTime(CalDateTime::Utc(
            NaiveDateTime::new(fixed_date(), NaiveTime::from_hms_opt(9, 0, 0).unwrap()).and_utc(),
        ));
        let end = CalDate::DateTime(CalDateTime::Utc(
            NaiveDateTime::new(
                NaiveDate::from_ymd_opt(2024, 6, 16).unwrap(),
                NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            )
            .and_utc(),
        ));
        let result = locale.date_range(Some(start), Some(end));
        assert_eq!(result, "Jun 15, 2024, 09:00 &#x2012; Jun 16, 2024, 17:00");
    }

    // --- Translations ---

    #[test]
    fn translations_default_is_empty() {
        let t = Translations::default();
        assert!(t.table.is_empty());
    }

    // --- default() function ---

    #[test]
    fn default_returns_locale_en() {
        let locale = crate::default();
        assert_eq!(locale.ty(), LocaleType::English);
    }
}
