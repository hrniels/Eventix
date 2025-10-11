use eventix_ical::objects::{CalAlarm, CalCompType, CalOrganizer};
use eventix_locale::Locale;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use xdg::BaseDirectories;

const FILENAME: &str = "settings.toml";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "calendar")]
    calendars: BTreeMap<String, CalendarSettings>,
}

impl Settings {
    pub fn locale(&self) -> Arc<dyn Locale + Send + Sync> {
        eventix_locale::default()
    }

    pub fn calendars(&self) -> &BTreeMap<String, CalendarSettings> {
        &self.calendars
    }

    pub fn calendar(&self, id: &String) -> Option<&CalendarSettings> {
        self.calendars.get(id)
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

    pub fn load_from_file(xdg: &BaseDirectories) -> anyhow::Result<Self> {
        match xdg.find_config_file(FILENAME) {
            Some(file) => super::load_from_file(&file),
            None => Ok(Settings::default()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailAccount {
    name: String,
    address: String,
}

impl EmailAccount {
    pub fn pretty_name(&self) -> String {
        format!("{} <{}>", self.name, self.address)
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn org_address(&self) -> &String {
        &self.address
    }

    pub fn address(&self) -> String {
        self.org_address().to_lowercase()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CalendarAlarmType {
    Calendar,
    Personal { default: Option<CalAlarm> },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SyncerType {
    FileSystem,
    VDirSyncer {
        name: String,
        local_name: String,
    },
    O365 {
        name: String,
        local_name: String,
        port: u16,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CalendarSettings {
    path: String,
    name: String,
    email: Option<EmailAccount>,
    fgcolor: String,
    bgcolor: String,
    types: Vec<CalCompType>,
    alarms: CalendarAlarmType,
    syncer: SyncerType,
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

    pub fn build_organizer(&self) -> Option<CalOrganizer> {
        self.email()
            .map(|em| CalOrganizer::new_named(em.name().to_string(), em.address()))
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

    pub fn alarms(&self) -> &CalendarAlarmType {
        &self.alarms
    }

    pub fn syncer(&self) -> &SyncerType {
        &self.syncer
    }
}
