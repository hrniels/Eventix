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

/// Locale errors
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

#[derive(Copy, Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum LocaleType {
    German,
    #[default]
    English,
}

#[derive(Default, Debug, Deserialize)]
pub struct Translations {
    pub table: HashMap<String, String>,
}

impl Translations {
    pub fn new_from_file(path: &Path) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).open(path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        Ok(toml::from_str(&content).unwrap())
    }
}

pub trait DateLike {
    fn naive(&self) -> NaiveDate;
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
    #[derive(Copy, Clone)]
    pub struct TimeFlags : u32{
        const None = 0;
        const Short = 1;
    }
}

bitflags! {
    #[derive(Copy, Clone)]
    pub struct DateFlags: u32 {
        const None = 0;
        const Short = 1;
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

pub trait Locale: CalLocale + Debug {
    fn ty(&self) -> LocaleType;

    fn translations(&self) -> &Translations;

    fn fmt_time(&self, date: &dyn DateLike, flags: TimeFlags) -> String {
        let fmt = if flags.contains(TimeFlags::Short) {
            "%H:%M"
        } else {
            "%H:%M:%S"
        };
        date.fmt(fmt)
    }

    fn fmt_weekdate(&self, date: &dyn DateLike, flags: DateFlags) -> String {
        let fmt = if flags.contains(DateFlags::Short) {
            "%a, %b %d"
        } else {
            "%A, %b %d %Y"
        };
        self.fmt_date_with(date, fmt, flags)
    }

    fn fmt_date(&self, date: &dyn DateLike, flags: DateFlags) -> String {
        let fmt = if flags.contains(DateFlags::Short) {
            "%b %d, %Y"
        } else {
            "%A, %B %d, %Y"
        };
        self.fmt_date_with(date, fmt, flags)
    }

    fn fmt_datetime(&self, date: &dyn DateLike, flags: DateFlags) -> String {
        format!(
            "{}, {}",
            self.fmt_date(date, flags),
            self.fmt_time(date, flags.into())
        )
    }

    fn fmt_date_with(&self, date: &dyn DateLike, f: &str, flags: DateFlags) -> String {
        if !flags.contains(DateFlags::NoToday)
            && let Some(rel) = self.has_relative(date)
        {
            return rel.to_string();
        }
        date.fmt(f).to_string()
    }

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

pub fn default() -> Arc<dyn Locale + Send + Sync> {
    Arc::new(LocaleEn::default())
}

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
