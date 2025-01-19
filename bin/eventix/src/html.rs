use std::time::Duration;

pub mod filters {
    use askama::{Html, MarkupDisplay};
    use chrono::{DateTime, NaiveDate, NaiveTime};
    use chrono_tz::Tz;
    use ical::objects::CalDate;
    use std::{fmt::Display, sync::Arc};

    use crate::locale::{DateFlags, DateLike, Locale, TimeFlags};

    pub fn deref<T: Clone>(value: &T) -> ::askama::Result<T> {
        Ok(value.clone())
    }

    pub fn as_time(time: super::Duration) -> ::askama::Result<String> {
        Ok(format!("{} µs", time.as_micros()))
    }

    pub fn newlines<T: Display>(text: T) -> ::askama::Result<String> {
        let text = MarkupDisplay::new_unsafe(text, Html);
        let text = format!("{}", text);
        Ok(text.replace('\n', "<br>"))
    }

    pub fn t<T: AsRef<str>>(
        text: T,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> ::askama::Result<String> {
        Ok(locale.translate(text.as_ref()).to_string())
    }

    pub fn naive_date(date: &Option<NaiveDate>) -> ::askama::Result<String> {
        Ok(match date {
            Some(d) => format!("{}", d.format("%Y-%m-%d")),
            None => String::new(),
        })
    }

    pub fn naive_time(date: &Option<NaiveTime>) -> ::askama::Result<String> {
        Ok(match date {
            Some(d) => format!("{}", d.format("%H:%M")),
            None => String::new(),
        })
    }

    pub fn time(
        date: &DateTime<Tz>,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: TimeFlags,
    ) -> ::askama::Result<String> {
        Ok(locale.fmt_time(date, flags))
    }

    pub fn weekdate(
        date: &dyn DateLike,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: DateFlags,
    ) -> ::askama::Result<String> {
        Ok(locale.fmt_weekdate(date, flags))
    }

    #[allow(dead_code)]
    pub fn caldate(
        date: &CalDate,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: DateFlags,
        end: bool,
    ) -> ::askama::Result<String> {
        let datetime = if end {
            date.as_end_with_tz(locale.timezone())
        } else {
            date.as_start_with_tz(locale.timezone())
        };
        match date {
            CalDate::Date(_) => Ok(locale.fmt_date(&datetime, flags)),
            CalDate::DateTime(_) => Ok(locale.fmt_datetime(&datetime, flags)),
        }
    }

    pub fn date(
        date: &dyn DateLike,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: DateFlags,
    ) -> ::askama::Result<String> {
        Ok(locale.fmt_date(date, flags))
    }

    pub fn datetime(
        date: &dyn DateLike,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: DateFlags,
    ) -> ::askama::Result<String> {
        Ok(locale.fmt_datetime(date, flags))
    }
}
