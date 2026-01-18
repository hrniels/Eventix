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
