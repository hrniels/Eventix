use askama::Template;
use eventix_ical::objects::CalCompType;
use eventix_locale::Locale;
use eventix_state::{CalendarAlarmType, CalendarSettings, CollectionSettings, SyncerType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use xdg::BaseDirectories;

use crate::{
    comps::alarmconfig::{AlarmConfig, AlarmConfigTemplate},
    html::filters,
};

pub enum CalendarBox<'a> {
    Known {
        id: &'a String,
        settings: &'a CalendarSettings,
    },
    Unknown {
        id: String,
        folder: String,
        name: String,
        color: String,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CalendarBoxMode {
    View,
    Edit,
}

impl<'a> CalendarBox<'a> {
    pub fn id(&self) -> String {
        match self {
            CalendarBox::Known { id, .. } => (*id).clone(),
            CalendarBox::Unknown { id, .. } => id.clone(),
        }
    }

    pub fn name(&self) -> &String {
        match self {
            CalendarBox::Known { settings, .. } => settings.name(),
            CalendarBox::Unknown { name, .. } => &name,
        }
    }

    pub fn bgcolor(&self) -> &String {
        match self {
            CalendarBox::Known { settings, .. } => settings.bgcolor(),
            CalendarBox::Unknown { color, .. } => &color,
        }
    }

    pub fn fgcolor(&self) -> String {
        match self {
            CalendarBox::Known { settings, .. } => settings.fgcolor().clone(),
            CalendarBox::Unknown { .. } => String::from("#000000"),
        }
    }

    pub fn folder(&self) -> &String {
        match self {
            CalendarBox::Known { settings, .. } => settings.folder(),
            CalendarBox::Unknown { folder, .. } => &folder,
        }
    }

    pub fn types(&self) -> &[CalCompType] {
        match self {
            CalendarBox::Known { settings, .. } => settings.types(),
            CalendarBox::Unknown { .. } => &[],
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AlarmType {
    Calendar,
    Personal,
}

impl From<&CalendarAlarmType> for AlarmType {
    fn from(ty: &CalendarAlarmType) -> Self {
        match ty {
            CalendarAlarmType::Calendar => Self::Calendar,
            CalendarAlarmType::Personal { .. } => Self::Personal,
        }
    }
}

#[derive(Template)]
#[template(path = "comps/calbox.htm")]
pub struct CalendarBoxTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    col_id: &'a String,
    col: &'a CollectionSettings,
    col_path: String,
    cal: CalendarBox<'a>,
    mode: CalendarBoxMode,
    alarm_type: AlarmType,
    personal: AlarmConfigTemplate,
}

impl<'a> CalendarBoxTemplate<'a> {
    pub fn new(
        xdg: &BaseDirectories,
        locale: Arc<dyn Locale + Send + Sync>,
        col_id: &'a String,
        col: &'a CollectionSettings,
        cal: CalendarBox<'a>,
        mode: CalendarBoxMode,
    ) -> Self {
        let (alarm_type, personal) = match cal {
            CalendarBox::Known { settings, .. } => {
                let personal = if let CalendarAlarmType::Personal { default } = settings.alarms() {
                    default.as_ref().and_then(|def| {
                        Some(AlarmConfig::from_alarms(&[def.clone()], locale.timezone()))
                    })
                } else {
                    None
                };
                (settings.alarms().into(), personal)
            }
            _ => (AlarmType::Calendar, None),
        };

        let col_path = col.path(xdg, col_id).to_str().unwrap().to_string();
        Self {
            col_id,
            col,
            col_path,
            cal,
            mode,
            alarm_type,
            personal: AlarmConfigTemplate::new(locale.clone(), String::from("alarms"), personal),
            locale,
        }
    }

    pub fn cal(&self) -> &CalendarBox<'a> {
        &self.cal
    }
}
