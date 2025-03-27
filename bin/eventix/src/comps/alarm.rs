use askama::Template;
use chrono_tz::Tz;
use ical::objects::CalAlarm;
use serde::Deserialize;
use std::sync::Arc;

use crate::pages::Page;
use crate::{
    comps::alarmconfig::AlarmConfig, comps::alarmconfig::AlarmConfigTemplate, html::filters,
    locale::Locale,
};

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

    pub fn check(&self, page: &mut Page, locale: &Arc<dyn Locale + Send + Sync>) -> bool {
        if !self.calendar.check(page, locale) {
            return false;
        }
        if let Some(personal) = &self.personal {
            personal.check(page, locale)
        } else {
            true
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn to_alarms(
        &self,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> anyhow::Result<(Option<Vec<CalAlarm>>, Option<Option<Vec<CalAlarm>>>)> {
        Ok((
            self.calendar.to_alarms(locale)?,
            match &self.personal {
                Some(pers) => Some(pers.to_alarms(locale)?),
                None => None,
            },
        ))
    }
}

#[derive(Template)]
#[template(path = "comps/alarm.htm")]
pub struct AlarmTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    name: &'a str,
    id: String,
    edit: bool,
    effective: Option<&'a Vec<CalAlarm>>,
    calendar: AlarmConfigTemplate,
    personal: AlarmConfigTemplate,
    personal_overwrite: bool,
}

impl<'a> AlarmTemplate<'a> {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        name: &'a str,
        edit: bool,
        effective: Option<&'a Vec<CalAlarm>>,
        value: AlarmRequest,
    ) -> Self {
        Self {
            name,
            edit,
            id: name.replace("[", "_").replace("]", "_"),
            effective,
            calendar: AlarmConfigTemplate::new(
                locale.clone(),
                format!("{}[calendar]", name),
                Some(value.calendar),
            ),
            personal_overwrite: value.personal.is_some(),
            personal: AlarmConfigTemplate::new(
                locale.clone(),
                format!("{}[personal]", name),
                value.personal,
            ),
            locale,
        }
    }
}
