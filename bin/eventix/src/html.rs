// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::objects::{CalAttendee, CalPartStat, CalRole};
use regex::{Captures, Regex};

pub mod filters {
    use askama::filters::Safe;
    use chrono::{NaiveDate, NaiveTime};
    use std::{fmt::Display, sync::Arc};

    use eventix_locale::{DateFlags, DateLike, Locale, TimeFlags};

    #[askama::filter_fn]
    pub fn newlines<T: Display>(
        text: T,
        _values: &dyn ::askama::Values,
    ) -> ::askama::Result<Safe<String>> {
        let text = format!("{text}");
        Ok(Safe(text.replace('\n', "<br>")))
    }

    #[askama::filter_fn]
    pub fn t<T: AsRef<str>>(
        text: T,
        _values: &dyn ::askama::Values,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> ::askama::Result<String> {
        Ok(locale.translate(text.as_ref()).to_string())
    }

    #[askama::filter_fn]
    pub fn naive_date(
        date: &Option<NaiveDate>,
        _values: &dyn ::askama::Values,
    ) -> ::askama::Result<String> {
        Ok(match date {
            Some(d) => format!("{}", d.format("%Y-%m-%d")),
            None => String::new(),
        })
    }

    #[askama::filter_fn]
    pub fn naive_time(
        date: &Option<NaiveTime>,
        _values: &dyn ::askama::Values,
    ) -> ::askama::Result<String> {
        Ok(match date {
            Some(d) => format!("{}", d.format("%H:%M")),
            None => String::new(),
        })
    }

    #[askama::filter_fn]
    pub fn time(
        date: &dyn DateLike,
        _values: &dyn ::askama::Values,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: TimeFlags,
    ) -> ::askama::Result<String> {
        Ok(locale.fmt_time(date, flags))
    }

    #[askama::filter_fn]
    pub fn weekdate(
        date: &dyn DateLike,
        _values: &dyn ::askama::Values,
        locale: &Arc<dyn Locale + Send + Sync>,
        flags: DateFlags,
    ) -> ::askama::Result<String> {
        Ok(locale.fmt_weekdate(date, flags))
    }

    #[askama::filter_fn]
    pub fn to_id<S: AsRef<str>>(id: S, _values: &dyn ::askama::Values) -> ::askama::Result<String> {
        Ok(super::to_id(id))
    }
}

pub fn to_id<S: AsRef<str>>(id: S) -> String {
    id.as_ref()
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            _ => '_',
        })
        .collect()
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
