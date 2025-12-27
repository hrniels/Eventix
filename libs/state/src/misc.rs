use chrono::NaiveDateTime;
use eventix_ical::objects::CalCompType;
use eventix_locale::LocaleType;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};
use xdg::BaseDirectories;

const FILENAME: &str = "misc.toml";

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Misc {
    #[serde(skip)]
    path: PathBuf,
    #[serde(default)]
    locale_type: LocaleType,
    #[serde(default)]
    last_alarm_check: NaiveDateTime,
    #[serde(default)]
    last_calendar: HashMap<CalCompType, String>,
    #[serde(default)]
    disabled_calendars: Vec<String>,
    #[serde(default)]
    sync_errors: HashSet<String>,
    #[serde(default)]
    calendar_tokens: HashMap<String, String>,
}

impl Misc {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            locale_type: LocaleType::default(),
            last_alarm_check: chrono::Local::now().naive_utc(),
            last_calendar: HashMap::default(),
            disabled_calendars: Vec::default(),
            sync_errors: HashSet::default(),
            calendar_tokens: HashMap::default(),
        }
    }

    pub fn locale_type(&self) -> LocaleType {
        self.locale_type
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

    pub fn has_sync_error(&self, id: &String) -> bool {
        self.sync_errors.contains(id)
    }

    pub fn set_sync_error(&mut self, id: &String, error: bool) {
        if self.has_sync_error(id) && !error {
            self.sync_errors.remove(id);
        } else if error {
            self.sync_errors.insert(id.clone());
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
