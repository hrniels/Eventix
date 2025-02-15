mod de;
mod en;

use bitflags::bitflags;
use chrono::{DateTime, NaiveDate, Utc};
pub use de::LocaleDe;
#[allow(unused_imports)]
pub use en::LocaleEn;

use chrono_tz::Tz;

use std::sync::Arc;

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
        self.fmt_date_with(date, "%A, %b %d", flags)
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

    fn translate<'a>(&self, key: &'a str) -> &'a str;
}

pub fn default() -> Arc<dyn Locale + Send + Sync> {
    Arc::new(LocaleDe::default())
}
