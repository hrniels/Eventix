use anyhow::anyhow;
use askama::Template;
use chrono::Duration;
use chrono_tz::Tz;
use ical::objects::{CalAction, CalAlarm, CalDateType, CalRelated, CalTrigger};
use serde::{Deserialize, Deserializer};
use std::fmt::{self, Display};
use std::sync::Arc;
use strum::EnumIter;

use crate::comps::{combobox::Named, datetime::DateTime, datetime::DateTimeTemplate};
use crate::html::filters;
use crate::locale::Locale;
use crate::pages::Page;

use super::combobox::ComboboxTemplate;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Trigger {
    Relative,
    Absolute,
}

impl Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Relative => write!(f, "RELATIVE"),
            Self::Absolute => write!(f, "ABSOLUTE"),
        }
    }
}

impl From<CalTrigger> for Trigger {
    fn from(value: CalTrigger) -> Self {
        match value {
            CalTrigger::Relative { .. } => Self::Relative,
            CalTrigger::Absolute(_) => Self::Absolute,
        }
    }
}

impl Trigger {
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Trigger>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf = String::deserialize(deserializer)?;
        match buf.as_str() {
            "RELATIVE" => Ok(Some(Trigger::Relative)),
            "ABSOLUTE" => Ok(Some(Trigger::Absolute)),
            _ => Ok(None),
        }
    }
}

#[derive(Default, Debug, Deserialize, PartialEq, Eq, EnumIter)]
pub enum DurUnit {
    #[default]
    Minutes,
    Hours,
    Days,
}

impl Named for DurUnit {
    fn name(&self) -> String {
        format!("{self:?}")
    }
}

impl Display for DurUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Default, Debug, Deserialize, PartialEq, Eq, EnumIter)]
pub enum DurType {
    #[default]
    BeforeStart,
    AfterStart,
    BeforeEnd,
    AfterEnd,
}

impl Display for DurType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Named for DurType {
    fn name(&self) -> String {
        match self {
            Self::BeforeStart => "Before start".to_string(),
            Self::AfterStart => "After start".to_string(),
            Self::BeforeEnd => "Before end".to_string(),
            Self::AfterEnd => "After end".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AlarmConfig {
    #[serde(deserialize_with = "Trigger::deserialize")]
    trigger: Option<Trigger>,
    duration: u64,
    durunit: DurUnit,
    durtype: DurType,
    datetime: Option<DateTime>,
}

impl Default for AlarmConfig {
    fn default() -> Self {
        Self {
            trigger: None,
            duration: 1,
            durunit: DurUnit::default(),
            durtype: DurType::default(),
            datetime: None,
        }
    }
}

fn translate(alarm: &CalAlarm, timezone: &Tz) -> (u64, DurUnit, DurType, Option<DateTime>) {
    let determine_type = |rel: &CalRelated, dur: &Duration| match rel {
        CalRelated::Start if *dur < Duration::zero() => DurType::BeforeStart,
        CalRelated::Start if *dur >= Duration::zero() => DurType::AfterStart,
        CalRelated::End if *dur < Duration::zero() => DurType::BeforeEnd,
        CalRelated::End if *dur >= Duration::zero() => DurType::AfterEnd,
        _ => unreachable!(),
    };

    match alarm.trigger() {
        CalTrigger::Relative { related, duration } => {
            if duration.num_days() != 0 {
                (
                    duration.num_days().unsigned_abs(),
                    DurUnit::Days,
                    determine_type(related, duration),
                    None,
                )
            } else if duration.num_hours() != 0 {
                (
                    duration.num_hours().unsigned_abs(),
                    DurUnit::Hours,
                    determine_type(related, duration),
                    None,
                )
            } else if duration.num_minutes() != 0 {
                (
                    duration.num_minutes().unsigned_abs(),
                    DurUnit::Minutes,
                    determine_type(related, duration),
                    None,
                )
            } else {
                (1, DurUnit::default(), DurType::default(), None)
            }
        }
        CalTrigger::Absolute(dt) => (
            1,
            DurUnit::default(),
            DurType::default(),
            Some(DateTime::from_caldate(dt, timezone)),
        ),
    }
}

impl AlarmConfig {
    pub fn from_alarms(alarm: &[CalAlarm], timezone: &Tz) -> Self {
        if let Some(a) = alarm.first() {
            let (duration, durunit, durtype, datetime) = translate(a, timezone);
            Self {
                trigger: Some(a.trigger().clone().into()),
                duration,
                durunit,
                durtype,
                datetime,
            }
        } else {
            Self::default()
        }
    }

    pub fn check(&self, page: &mut Page, locale: &Arc<dyn Locale + Send + Sync>) -> bool {
        if let Some(Trigger::Absolute) = self.trigger {
            if self
                .datetime
                .as_ref()
                .and_then(|dt| dt.to_caldate(locale, CalDateType::Inclusive, false))
                .is_none()
            {
                page.add_error(locale.translate("Please specify a valid date and time"));
                return false;
            }
        }
        true
    }

    pub fn to_alarms(
        &self,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> anyhow::Result<Option<Vec<CalAlarm>>> {
        if let Some(trigger) = self.trigger {
            let duration = match self.durtype {
                DurType::BeforeStart | DurType::BeforeEnd => -(self.duration as i64),
                _ => self.duration as i64,
            };
            let trigger = match trigger {
                Trigger::Relative => CalTrigger::Relative {
                    related: match self.durtype {
                        DurType::BeforeStart | DurType::AfterStart => CalRelated::Start,
                        _ => CalRelated::End,
                    },
                    duration: match self.durunit {
                        DurUnit::Days => Duration::days(duration).into(),
                        DurUnit::Hours => Duration::hours(duration).into(),
                        DurUnit::Minutes => Duration::minutes(duration).into(),
                    },
                },
                Trigger::Absolute => CalTrigger::Absolute(
                    match self.datetime {
                        Some(ref dt) => dt.to_caldate(locale, CalDateType::Inclusive, false),
                        None => None,
                    }
                    .ok_or_else(|| anyhow!("Invalid datetime"))?
                    .to_utc(),
                ),
            };
            let alarm = CalAlarm::new(CalAction::Display, trigger);
            Ok(Some(vec![alarm]))
        } else {
            Ok(None)
        }
    }
}

#[derive(Template)]
#[template(path = "comps/alarmconfig.htm")]
pub struct AlarmConfigTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
    name: String,
    id: String,
    trigger: String,
    duration: u64,
    durunit: ComboboxTemplate<DurUnit>,
    durtype: ComboboxTemplate<DurType>,
    datetime: DateTimeTemplate,
}

impl AlarmConfigTemplate {
    pub fn new(
        locale: Arc<dyn Locale + Send + Sync>,
        name: String,
        value: Option<AlarmConfig>,
    ) -> Self {
        let value = value.unwrap_or_default();
        Self {
            trigger: match value.trigger {
                Some(f) => format!("{f}"),
                None => String::from("NONE"),
            },
            duration: value.duration,
            durunit: ComboboxTemplate::new(
                locale.clone(),
                format!("{}[durunit]", &name),
                Some(value.durunit),
            ),
            durtype: ComboboxTemplate::new(
                locale.clone(),
                format!("{}[durtype]", &name),
                Some(value.durtype),
            ),
            datetime: DateTimeTemplate::new(format!("{name}[datetime]"), value.datetime),
            id: name.replace("[", "_").replace("]", "_"),
            name,
            locale,
        }
    }
}
