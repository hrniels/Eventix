use chrono::NaiveDateTime;
use ical::objects::CalCompType;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};

const FILENAME: &str = "data/misc.toml";

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Misc {
    last_alarm_check: NaiveDateTime,
    last_calendar: HashMap<CalCompType, String>,
    disabled_calendars: Vec<String>,
}

impl Default for Misc {
    fn default() -> Self {
        Self {
            last_alarm_check: chrono::Local::now().naive_utc(),
            last_calendar: HashMap::default(),
            disabled_calendars: Vec::default(),
        }
    }
}

impl Misc {
    pub fn last_alarm_check(&self) -> NaiveDateTime {
        self.last_alarm_check
    }

    pub fn set_last_alarm_check(&mut self, datetime: NaiveDateTime) {
        self.last_alarm_check = datetime;
    }

    pub fn last_calendar(&self, ty: CalCompType) -> Option<&String> {
        self.last_calendar.get(&ty)
    }

    pub fn set_last_calendar(&mut self, ty: CalCompType, cal: String) {
        if let Some(e) = self.last_calendar.get_mut(&ty) {
            *e = cal;
        } else {
            self.last_calendar.insert(ty, cal);
        }
    }

    pub fn calendar_disabled(&self, id: &String) -> bool {
        self.disabled_calendars.contains(id)
    }

    pub fn toggle_calendar(&mut self, id: &String) {
        if self.disabled_calendars.contains(id) {
            self.disabled_calendars.retain(|c| c != id);
        } else {
            self.disabled_calendars.push(id.to_string());
        }
    }

    pub fn load_from_file() -> anyhow::Result<Self> {
        let path: PathBuf = FILENAME.into();
        if fs::exists(&path)? {
            super::load_from_file(&path)
        } else {
            Ok(Self::default())
        }
    }

    pub fn write_to_file(&self) -> anyhow::Result<()> {
        super::write_to_file(&FILENAME.into(), self)
    }
}
