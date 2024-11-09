use std::time::Duration;

pub mod filters {
    use askama::{Html, MarkupDisplay};
    use chrono::{DateTime, NaiveDate, NaiveTime};
    use chrono_tz::Tz;
    use std::{fmt::Display, sync::Arc};

    use crate::locale::Locale;

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

    pub fn date(
        date: &DateTime<Tz>,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> ::askama::Result<String> {
        Ok(locale.format_date(date))
    }

    pub fn datetime(
        date: &DateTime<Tz>,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> ::askama::Result<String> {
        Ok(locale.format_datetime(date))
    }
}
