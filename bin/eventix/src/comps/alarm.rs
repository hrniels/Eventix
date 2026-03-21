// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use askama::Template;
use chrono_tz::Tz;
use eventix_ical::objects::CalAlarm;
use eventix_locale::Locale;
use serde::Deserialize;
use std::sync::Arc;

use crate::comps::{alarmconfig::AlarmConfig, alarmconfig::AlarmConfigTemplate};
use crate::html::filters;
use crate::pages::Page;

#[derive(Default, Debug, Deserialize)]
pub struct AlarmRequest {
    calendar: AlarmConfig,
    personal: Option<AlarmConfig>,
}

impl AlarmRequest {
    pub fn from_alarms(
        calendar: &[CalAlarm],
        personal: Option<&[CalAlarm]>,
        timezone: &Tz,
    ) -> Self {
        Self {
            calendar: AlarmConfig::from_alarms(calendar, timezone),
            personal: personal.map(|cfg| AlarmConfig::from_alarms(cfg, timezone)),
        }
    }

    pub fn check(
        &self,
        page: &mut Page,
        locale: &Arc<dyn Locale + Send + Sync>,
        event_tz: &str,
    ) -> bool {
        if !self.calendar.check(page, locale, event_tz) {
            return false;
        }
        if let Some(personal) = &self.personal {
            personal.check(page, locale, event_tz)
        } else {
            true
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn to_alarms(
        &self,
        event_tz: &str,
    ) -> anyhow::Result<(Option<Vec<CalAlarm>>, Option<Option<Vec<CalAlarm>>>)> {
        Ok((
            self.calendar.to_alarms(event_tz)?,
            match &self.personal {
                Some(pers) => Some(pers.to_alarms(event_tz)?),
                None => None,
            },
        ))
    }
}

pub struct PersonalAlarms<'a> {
    config: AlarmConfigTemplate,
    overwrite: bool,
    effective: Option<&'a Vec<CalAlarm>>,
}

#[derive(Template)]
#[template(path = "comps/alarm.htm")]
pub struct AlarmTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    name: &'a str,
    id: String,
    edit: bool,
    calendar: AlarmConfigTemplate,
    personal: Option<PersonalAlarms<'a>>,
}

impl<'a> AlarmTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        name: &'a str,
        edit: bool,
        personal: bool,
        effective: Option<&'a Vec<CalAlarm>>,
        value: AlarmRequest,
    ) -> Self {
        Self {
            name,
            edit,
            id: name.replace("[", "_").replace("]", "_"),
            calendar: AlarmConfigTemplate::new(
                locale.clone(),
                format!("{name}[calendar]"),
                Some(value.calendar),
                true,
            ),
            personal: if personal {
                Some(PersonalAlarms {
                    effective,
                    overwrite: value.personal.is_some(),
                    config: AlarmConfigTemplate::new(
                        locale.clone(),
                        format!("{name}[personal]"),
                        value.personal,
                        true,
                    ),
                })
            } else {
                None
            },
            locale,
        }
    }
}
