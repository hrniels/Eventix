use eventix_ical::objects::{CalAttendee, CalDate, CalPartStat, CalRole};
use regex::{Captures, Regex};
use std::{sync::Arc, time::Duration};

use crate::locale::{DateFlags, Locale, TimeFlags};

pub mod filters {
    use askama::filters::Safe;
    use chrono::{DateTime, NaiveDate, NaiveTime};
    use chrono_tz::Tz;
    use eventix_ical::objects::CalDate;
    use std::{fmt::Display, sync::Arc};

    use crate::locale::{DateFlags, DateLike, Locale, TimeFlags};

    pub fn as_time(
        time: super::Duration,
        _values: &dyn ::askama::Values,
    ) -> ::askama::Result<String> {
        Ok(format!("{} µs", time.as_micros()))
    }

    pub fn newlines<T: Display>(
        text: T,
        _values: &dyn ::askama::Values,
    ) -> ::askama::Result<Safe<String>> {
        let text = format!("{text}");
        Ok(Safe(text.replace('\n', "<br>")))
    }

    pub fn t<T: AsRef<str>>(
        text: T,
        _values: &dyn ::askama::Values,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> ::askama::Result<String> {
        Ok(locale.translate(text.as_ref()).to_string())
    }

    pub fn naive_date(
        date: &Option<NaiveDate>,
        _values: &dyn ::askama::Values,
    ) -> ::askama::Result<String> {
        Ok(match date {
            Some(d) => format!("{}", d.format("%Y-%m-%d")),
            None => String::new(),
        })
    }

    pub fn naive_time(
        date: &Option<NaiveTime>,
        _values: &dyn ::askama::Values,
    ) -> ::askama::Result<String> {
        Ok(match date {
            Some(d) => format!("{}", d.format("%H:%M")),
            None => String::new(),
        })
    }

    pub fn time(
        date: &DateTime<Tz>,
        _values: &dyn ::askama::Values,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: TimeFlags,
    ) -> ::askama::Result<String> {
        Ok(locale.fmt_time(date, flags))
    }

    pub fn weekdate(
        date: &dyn DateLike,
        _values: &dyn ::askama::Values,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: DateFlags,
    ) -> ::askama::Result<String> {
        Ok(locale.fmt_weekdate(date, flags))
    }

    #[allow(dead_code)]
    pub fn caldate(
        date: &CalDate,
        _values: &dyn ::askama::Values,
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
            CalDate::Date(..) => Ok(locale.fmt_date(&datetime, flags)),
            CalDate::DateTime(_) => Ok(locale.fmt_datetime(&datetime, flags)),
        }
    }

    pub fn date(
        date: &dyn DateLike,
        _values: &dyn ::askama::Values,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: DateFlags,
    ) -> ::askama::Result<String> {
        Ok(locale.fmt_date(date, flags))
    }

    pub fn datetime(
        date: &dyn DateLike,
        _values: &dyn ::askama::Values,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: DateFlags,
    ) -> ::askama::Result<String> {
        Ok(locale.fmt_datetime(date, flags))
    }
}

pub fn text_to_html(text: Option<&String>) -> Option<String> {
    match text.map(|t| t.trim()) {
        Some(text) if !text.is_empty() => {
            // the problem is that we need to find URLs before translating HTML entities. but
            // if we directly replace URLs with '<a ...>', we will translate the HTML entities
            // in there afterwards. therefore, we use an intermediate step by first marking the
            // URLs by surrounding them with \0 and then we replace this with the actual HTML
            // code later.
            let regex = r"(https?:\/\/)?(www\.)?[-a-zA-Z0-9@:%._\+~#=]{1,256}\.[a-zA-Z0-9()]{2,6}\b([-a-zA-Z0-9()@:;%_\+.~#?&/=]*)";
            let re = Regex::new(regex).unwrap();
            let desc = re.replace_all(text, "\0$0\0");

            // now replace HTML entities etc.
            let text = format!("{desc}");
            let text = text.replace('\n', "<br>");

            // finally replace URLs with proper links
            let re = Regex::new("\0(.*?)\0").unwrap();
            Some(
                re.replace_all(&text, |caps: &Captures| {
                    // a few heuristics here to prefix URLs with the right protocol
                    if caps[1].starts_with("http:")
                        || caps[1].starts_with("https:")
                        || caps[1].starts_with("mailto:")
                    {
                        format!("<a href=\"{0}\">{0}</a>", &caps[1])
                    } else if caps[1].contains('@') {
                        format!("<a href=\"mailto:{0}\">{0}</a>", &caps[1])
                    } else {
                        format!("<a href=\"https://{0}\">{0}</a>", &caps[1])
                    }
                })
                .to_string(),
            )
        }
        _ => None,
    }
}

pub fn date_range(
    locale: &Arc<dyn Locale + Send + Sync>,
    start: Option<&CalDate>,
    end: Option<&CalDate>,
) -> String {
    let tz = locale.timezone();
    let date_flags = DateFlags::Short;
    let time_flags = TimeFlags::Short;
    match (start, end) {
        (Some(CalDate::Date(start, ..)), Some(CalDate::Date(end, ..)))
            if start.succ_opt() == Some(*end) =>
        {
            locale.fmt_date(&start, date_flags).to_string()
        }
        (Some(CalDate::Date(start, ..)), Some(end @ CalDate::Date(..))) => {
            format!(
                "{} &#x2012; {}",
                locale.fmt_date(&start, date_flags),
                locale.fmt_date(&end.as_end_with_tz(tz), date_flags)
            )
        }
        (Some(start), Some(end)) if start.as_naive_date() == end.as_naive_date() => {
            format!(
                "{}, {} &#x2012; {}",
                locale.fmt_date(&start.as_naive_date(), date_flags),
                locale.fmt_time(&start.as_start_with_tz(tz), time_flags),
                locale.fmt_time(&end.as_end_with_tz(tz), time_flags)
            )
        }
        (Some(start), Some(end)) => {
            format!(
                "{} &#x2012; {}",
                locale.fmt_datetime(&start.as_start_with_tz(tz), date_flags),
                locale.fmt_datetime(&end.as_end_with_tz(tz), date_flags)
            )
        }
        (Some(CalDate::Date(start, ..)), None) => locale.fmt_date(&start, date_flags),
        (Some(start @ CalDate::DateTime(_)), None) => {
            locale.fmt_datetime(&start.as_start_with_tz(tz), date_flags)
        }
        (None, Some(CalDate::Date(end, ..))) => locale.fmt_date(&end, date_flags),
        (None, Some(end @ CalDate::DateTime(_))) => {
            locale.fmt_datetime(&end.as_end_with_tz(tz), date_flags)
        }
        (None, None) => String::from("-"),
    }
}

pub fn attendee_icon(att: &CalAttendee) -> String {
    let role = match att.role() {
        Some(CalRole::Required) => "-fill",
        Some(CalRole::Optional) => "",
        _ => "",
    };

    let status = match att.part_stat() {
        Some(CalPartStat::Accepted) => "-check",
        Some(CalPartStat::Declined) => "-slash",
        Some(CalPartStat::Tentative) => "-exclamation",
        _ => "",
    };

    format!("bi bi-person{role}{status}")
}

pub fn attendee_title(att: &CalAttendee) -> String {
    let mut res = String::new();
    if let Some(role) = att.role() {
        res.push_str(&format!("{role:?}"));
    }
    if let Some(status) = att.part_stat() {
        if att.role().is_some() {
            res.push_str(", ");
        }
        res.push_str(&format!("{status:?}"));
    }
    res
}
