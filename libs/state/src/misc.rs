use chrono::NaiveDateTime;
use eventix_ical::objects::CalCompType;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use xdg::BaseDirectories;

const FILENAME: &str = "misc.toml";

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Misc {
    #[serde(skip)]
    path: PathBuf,
    #[serde(default)]
    last_alarm_check: NaiveDateTime,
    #[serde(default)]
    last_calendar: HashMap<CalCompType, String>,
    #[serde(default)]
    disabled_calendars: Vec<String>,
    #[serde(default)]
    sync_errors: HashMap<String, String>,
    #[serde(default)]
    calendar_tokens: HashMap<String, String>,
}

impl Misc {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            last_alarm_check: chrono::Local::now().naive_utc(),
            last_calendar: HashMap::default(),
            disabled_calendars: Vec::default(),
            sync_errors: HashMap::default(),
            calendar_tokens: HashMap::default(),
        }
    }

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

    pub fn get_sync_error(&self, id: &String) -> Option<&String> {
        self.sync_errors.get(id)
    }

    pub fn set_sync_error(&mut self, id: &String, error: Option<String>) {
        if let Some(error) = error {
            *self.sync_errors.entry(id.to_string()).or_default() = error;
        } else {
            self.sync_errors.remove(id);
        }
    }

    pub fn calendar_token(&self, id: &String) -> Option<&String> {
        self.calendar_tokens.get(id)
    }

    pub fn set_calendar_token(&mut self, id: &String, token: String) {
        *self.calendar_tokens.entry(id.to_string()).or_default() = token;
    }

    pub fn load_from_file(xdg: &BaseDirectories) -> anyhow::Result<Self> {
        match xdg.find_data_file(FILENAME) {
            Some(path) => {
                let mut misc: Self = super::load_from_file(&path)?;
                misc.path = path;
                Ok(misc)
            }
            None => {
                let path = xdg.place_data_file(FILENAME)?;
                Ok(Self::new(path))
            }
        }
    }

    pub fn write_to_file(&self) -> anyhow::Result<()> {
        super::write_to_file(&self.path, self)
    }
}
