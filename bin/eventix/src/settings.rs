use anyhow::Context;
use chrono::NaiveDateTime;
use ical::objects::CalCompType;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{Read, Write},
};
use tokio::sync::Mutex;

const FILENAME: &str = "settings.toml";
static MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "calendar")]
    calendars: BTreeMap<String, CalendarSettings>,
    last_alarm_check: NaiveDateTime,
    last_calendar: HashMap<CalCompType, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            calendars: BTreeMap::default(),
            last_alarm_check: chrono::Local::now().naive_utc(),
            last_calendar: HashMap::default(),
        }
    }
}

impl Settings {
    pub fn calendars(&self) -> &BTreeMap<String, CalendarSettings> {
        &self.calendars
    }

    pub fn calendar(&self, id: &String) -> Option<&CalendarSettings> {
        self.calendars.get(id)
    }

    pub fn calendar_disabled(&self, id: &String) -> bool {
        self.calendars.get(id).unwrap().disabled
    }

    pub fn toggle_calendar(&mut self, id: &String) {
        let cal = self.calendars.get_mut(id).unwrap();
        cal.disabled = !cal.disabled;
    }

    pub fn emails(&self) -> HashMap<String, String> {
        let mut res = HashMap::new();
        for (id, settings) in &self.calendars {
            if let Some(email) = settings.email() {
                res.insert(id.clone(), email.pretty_name());
            }
        }
        res
    }

    pub fn last_alarm_check(&self) -> NaiveDateTime {
        self.last_alarm_check
    }

    pub fn set_last_alarm_check(&mut self, datetime: NaiveDateTime) {
        self.last_alarm_check = datetime;
    }

    pub fn last_calendar(&self, ty: CalCompType) -> &String {
        self.last_calendar.get(&ty).unwrap()
    }

    pub fn set_last_calendar(&mut self, ty: CalCompType, cal: String) {
        if let Some(e) = self.last_calendar.get_mut(&ty) {
            *e = cal;
        } else {
            self.last_calendar.insert(ty, cal);
        }
    }

    pub async fn load_from_file() -> anyhow::Result<Self> {
        // ensure that reads/writes to this file do not happen in parallel
        let _guard = MUTEX.lock().await;
        let mut file = File::options()
            .read(true)
            .open(FILENAME)
            .context(format!("open {}", FILENAME))?;
        let mut dirs = String::new();
        file.read_to_string(&mut dirs)
            .context(format!("read {}", FILENAME))?;
        toml::from_str(&dirs).context(format!("parse {}", FILENAME))
    }

    pub async fn write_to_file(&self) -> anyhow::Result<()> {
        let _guard = MUTEX.lock().await;
        let mut file = File::options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(FILENAME)
            .context(format!("open {}", FILENAME))?;
        file.write_all(
            toml::to_string(self)
                .context("serialize settings")?
                .as_bytes(),
        )
        .context("write settings")?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmailAccount {
    name: String,
    address: String,
}

impl EmailAccount {
    pub fn pretty_name(&self) -> String {
        format!("{} <{}>", self.name, self.address)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CalendarSettings {
    path: String,
    name: String,
    email: Option<EmailAccount>,
    disabled: bool,
    fgcolor: String,
    bgcolor: String,
    types: Vec<CalCompType>,
}

impl CalendarSettings {
    pub fn path(&self) -> &String {
        &self.path
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn email(&self) -> Option<&EmailAccount> {
        self.email.as_ref()
    }

    pub fn disabled(&self) -> bool {
        self.disabled
    }

    pub fn types(&self) -> &[CalCompType] {
        &self.types
    }

    pub fn fgcolor(&self) -> &String {
        &self.fgcolor
    }

    pub fn bgcolor(&self) -> &String {
        &self.bgcolor
    }
}
