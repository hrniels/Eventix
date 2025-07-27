mod de;
mod en;

use bitflags::bitflags;
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use eventix_ical::objects::CalDate;
use std::sync::Arc;

pub use de::LocaleDe;
#[allow(unused_imports)]
pub use en::LocaleEn;

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

pub trait Locale {
    fn timezone(&self) -> &Tz {
        &chrono_tz::Europe::Berlin
    }

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
        let fmt = if flags.contains(DateFlags::Short) {
            "%b %d, %Y"
        } else {
            "%A, %B %d, %Y"
        };
        let prefix = self.fmt_date_with(date, fmt, flags);
        format!("{}, {}", prefix, self.fmt_time(date, flags.into()))
    }

    fn fmt_date_with(&self, date: &dyn DateLike, f: &str, flags: DateFlags) -> String {
        if !flags.contains(DateFlags::NoToday) {
            let today = Utc::now().date_naive();
            if date.naive() == today {
                return String::from("Today");
            } else if Some(date.naive()) == today.succ_opt() {
                return String::from("Tomorrow");
            } else if Some(date.naive()) == today.pred_opt() {
                return String::from("Yesterday");
            }
        }
        date.fmt(f).to_string()
    }

    fn date_range(&self, start: Option<&CalDate>, end: Option<&CalDate>) -> String {
        let tz = self.timezone();
        let date_flags = DateFlags::Short;
        let time_flags = TimeFlags::Short;
        match (start, end) {
            (Some(CalDate::Date(start, ..)), Some(CalDate::Date(end, ..)))
                if start.succ_opt() == Some(*end) =>
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

    fn translate<'a>(&self, key: &'a str) -> &'a str;
}

pub fn default() -> Arc<dyn Locale + Send + Sync> {
    Arc::new(LocaleDe::default())
}
