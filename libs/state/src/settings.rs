use eventix_ical::objects::{CalAlarm, CalCompType, CalOrganizer};
use eventix_locale::Locale;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
};
use xdg::BaseDirectories;

const FILENAME: &str = "settings.toml";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "collection")]
    collections: BTreeMap<String, CollectionSettings>,
}

impl Settings {
    pub fn locale(&self) -> Arc<dyn Locale + Send + Sync> {
        eventix_locale::default()
    }

    pub fn collections(&self) -> &BTreeMap<String, CollectionSettings> {
        &self.collections
    }

    pub fn calendars(&self) -> impl Iterator<Item = (&String, &CalendarSettings)> {
        self.collections
            .values()
            .flat_map(|col| col.calendars.iter())
    }

    pub fn calendar(&self, id: &String) -> Option<(&CollectionSettings, &CalendarSettings)> {
        for col in self.collections.values() {
            if let Some(settings) = col.calendars.get(id) {
                return Some((col, settings));
            }
        }
        None
    }

    pub fn emails(&self) -> HashMap<String, String> {
        let mut res = HashMap::new();
        for col in self.collections.values() {
            if let Some(email) = col.email() {
                for id in col.calendars.keys() {
                    res.insert(id.clone(), email.pretty_name());
                }
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
    FileSystem {
        path: String,
    },
    VDirSyncer {
        url: String,
        read_only: bool,
        password_cmd: Vec<String>,
    },
    O365 {
        read_only: bool,
        password_cmd: Vec<String>,
    },
}

impl SyncerType {
    pub fn path(&self, xdg: &BaseDirectories, name: &String) -> PathBuf {
        match self {
            Self::FileSystem { path } => PathBuf::from(path),
            Self::VDirSyncer { .. } | Self::O365 { .. } => xdg
                .get_data_file("vdirsyncer")
                .unwrap()
                .join(format!("{}-data", name)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CollectionSettings {
    email: Option<EmailAccount>,
    syncer: SyncerType,
    #[serde(rename = "calendar")]
    calendars: BTreeMap<String, CalendarSettings>,
}

impl CollectionSettings {
    pub fn path(&self, xdg: &BaseDirectories, name: &String) -> PathBuf {
        self.syncer.path(xdg, name)
    }

    pub fn email(&self) -> Option<&EmailAccount> {
        self.email.as_ref()
    }

    pub fn build_organizer(&self) -> Option<CalOrganizer> {
        self.email()
            .map(|em| CalOrganizer::new_named(em.name().to_string(), em.address()))
    }

    pub fn syncer(&self) -> &SyncerType {
        &self.syncer
    }

    pub fn calendars(&self) -> impl Iterator<Item = (&String, &CalendarSettings)> {
        self.calendars.iter()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CalendarSettings {
    folder: String,
    name: String,
    fgcolor: String,
    bgcolor: String,
    types: Vec<CalCompType>,
    alarms: CalendarAlarmType,
}

impl CalendarSettings {
    pub fn folder(&self) -> &String {
        &self.folder
    }

    pub fn name(&self) -> &String {
        &self.name
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
}
