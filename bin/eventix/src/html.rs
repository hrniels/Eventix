use std::time::Duration;

pub mod filters {
    use askama::{Html, MarkupDisplay};
    use chrono::NaiveDate;
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

    pub fn ts<T: ToString>(
        text: T,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> ::askama::Result<String> {
        Ok(locale.translate(&text.to_string()).to_string())
    }

    pub fn date(
        date: &NaiveDate,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> ::askama::Result<String> {
        Ok(locale.format_date(date))
    }

    pub fn neutral_date(date: &Option<NaiveDate>) -> ::askama::Result<String> {
        Ok(match date {
            Some(d) => format!("{}", d.format("%Y-%m-%d")),
            None => String::new(),
        })
    }

    pub fn currency(num: &f64, locale: &Arc<dyn Locale + Send + Sync>) -> ::askama::Result<String> {
        Ok(locale.format_currency(*num))
    }
}

pub fn human_list<T>(objs: &[T]) -> String
where
    T: AsRef<str>,
{
    if objs.len() > 1 {
        let start = itertools::join(
            objs.iter()
                .take(objs.len() - 1)
                .map(|o| format!("'{}'", o.as_ref())),
            ", ",
        );
        format!("{}, and '{}'", start, objs.last().unwrap().as_ref())
    } else {
        itertools::join(objs.iter().map(|o| format!("'{}'", o.as_ref())), ", ")
    }
}
