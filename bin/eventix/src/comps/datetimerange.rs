// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use chrono::NaiveDate;
use eventix_ical::col::Occurrence;
use eventix_ical::objects::{CalCompType, CalDate, CalDateType};
use eventix_locale::Locale;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::comps::date::Date;
use crate::comps::datetime::DateTime;
use crate::comps::time::{Time, TimeTemplate};
use crate::comps::tzcombo::TzComboTemplate;
use crate::html::filters;

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct DateTimeRange {
    from: DateTime,
    to: DateTime,
    from_enabled: Option<bool>,
    to_enabled: Option<bool>,
    timezone: Option<String>,
}

impl DateTimeRange {
    pub fn new(from: DateTime, to: DateTime, timezone: String) -> Self {
        Self {
            from_enabled: from.date().map(|_| true),
            to_enabled: to.date().map(|_| true),
            from,
            to,
            timezone: Some(timezone),
        }
    }

    pub fn new_from_occurrence(occ: &Occurrence<'_>) -> Self {
        let from = occ.resolved_occurrence_start();
        let to = occ.resolved_occurrence_end();

        Self {
            from: DateTime::new(
                Date::new(from.as_ref().map(|f| f.date_naive())),
                occ.occurrence_startdate().and_then(|f| match f {
                    CalDate::DateTime(_) => Some(Time::new(from.as_ref().unwrap().time())),
                    CalDate::Date(..) => None,
                }),
            ),
            to: DateTime::new(
                Date::new(to.as_ref().map(|t| t.date_naive())),
                occ.occurrence_enddate().and_then(|t| match t {
                    CalDate::DateTime(_) => Some(Time::new(to.as_ref().unwrap().time())),
                    CalDate::Date(..) => None,
                }),
            ),
            from_enabled: from.map(|_| true),
            to_enabled: to.map(|_| true),
            timezone: occ.tz_name(),
        }
    }

    pub fn is_all_day(&self) -> bool {
        self.from.time().is_none() && self.to.time().is_none()
    }

    /// Returns the timezone name stored in this range, falling back to the
    /// locale timezone if none was set.
    pub fn effective_timezone(&self, locale: &Arc<dyn Locale + Send + Sync>) -> String {
        self.timezone
            .clone()
            .unwrap_or_else(|| locale.timezone().name().to_string())
    }

    pub fn as_caldates(
        &self,
        locale: &Arc<dyn Locale + Send + Sync>,
        ty: CalDateType,
    ) -> (Option<CalDate>, Option<CalDate>) {
        let tz = self.effective_timezone(locale);
        (
            if self.from_enabled.is_none() {
                None
            } else {
                self.from.to_caldate(&tz, ty, false)
            },
            if self.to_enabled.is_none() {
                None
            } else {
                self.to.to_caldate(&tz, ty, true)
            },
        )
    }
}

#[derive(Template)]
#[template(path = "comps/datetimerange.htm")]
pub struct DateTimeRangeTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    ctype: CalCompType,
    name: &'a str,
    id: String,
    from_date: Option<NaiveDate>,
    to_date: Option<NaiveDate>,
    from_time: TimeTemplate,
    to_time: TimeTemplate,
    from_enabled: bool,
    to_enabled: bool,
    all_day: bool,
    tz_combo: TzComboTemplate,
}

impl<'a> DateTimeRangeTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        ctype: CalCompType,
        name: &'a str,
        value: Option<DateTimeRange>,
    ) -> Self {
        let tz_name = value
            .as_ref()
            .and_then(|v| v.timezone.clone())
            .unwrap_or_else(|| locale.timezone().name().to_string());
        let tz_combo = TzComboTemplate::new(locale.clone(), format!("{name}[timezone]"), tz_name);
        Self {
            locale,
            ctype,
            name,
            id: name.replace(['[', ']'], "_"),
            from_date: value.as_ref().and_then(|v| v.from.date()),
            to_date: value.as_ref().and_then(|v| v.to.date()),
            from_time: TimeTemplate::new(
                format!("{name}[from][time]"),
                value.as_ref().and_then(|v| v.from.time()),
            ),
            to_time: TimeTemplate::new(
                format!("{name}[to][time]"),
                value.as_ref().and_then(|v| v.to.time()),
            ),
            all_day: value.as_ref().map(|v| v.is_all_day()).unwrap_or(false),
            from_enabled: value
                .as_ref()
                .map(|v| v.from_enabled.is_some())
                .unwrap_or(false),
            to_enabled: value
                .as_ref()
                .map(|v| v.to_enabled.is_some())
                .unwrap_or(false),
            tz_combo,
        }
    }
}
