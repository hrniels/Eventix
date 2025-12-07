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

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(skip)]
    path: PathBuf,
    #[serde(rename = "collection")]
    collections: BTreeMap<String, CollectionSettings>,
}

impl Settings {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            collections: BTreeMap::default(),
        }
    }

    pub fn locale(&self) -> Arc<dyn Locale + Send + Sync> {
        eventix_locale::default()
    }

    pub fn collections(&self) -> &BTreeMap<String, CollectionSettings> {
        &self.collections
    }

    pub fn collections_mut(&mut self) -> &mut BTreeMap<String, CollectionSettings> {
        &mut self.collections
    }

    pub fn calendars(&self) -> impl Iterator<Item = (&String, &CalendarSettings)> {
        self.collections
            .values()
            .flat_map(|col| col.calendars.iter().filter(|(_, c)| c.enabled()))
    }

    pub fn calendar(&self, id: &String) -> Option<(&CollectionSettings, &CalendarSettings)> {
        for col in self.collections.values() {
            if let Some(settings) = col.calendars.get(id) {
                if settings.enabled() {
                    return Some((col, settings));
                } else {
                    return None;
                }
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
            Some(file) => {
                let mut settings: Self = super::load_from_file(&file)?;
                settings.path = file;
                Ok(settings)
            }
            None => Ok(Settings::new(PathBuf::from(FILENAME))),
        }
    }

    pub fn write_to_file(&self) -> anyhow::Result<()> {
        super::write_to_file(&self.path, self)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailAccount {
    name: String,
    address: String,
}

impl EmailAccount {
    pub fn new(name: String, address: String) -> Self {
        Self { name, address }
    }

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

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub enum CalendarAlarmType {
    #[default]
    Calendar,
    Personal {
        default: Option<CalAlarm>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SyncerType {
    FileSystem {
        path: String,
    },
    VDirSyncer {
        email: EmailAccount,
        url: String,
        read_only: bool,
        username: Option<String>,
        password_cmd: Option<Vec<String>>,
    },
    O365 {
        email: EmailAccount,
        read_only: bool,
        password_cmd: Vec<String>,
    },
}

impl SyncerType {
    pub fn email(&self) -> Option<&EmailAccount> {
        match self {
            Self::VDirSyncer { email, .. } => Some(email),
            Self::O365 { email, .. } => Some(email),
            _ => None,
        }
    }

    pub fn supports_discover(&self) -> bool {
        matches!(self, Self::VDirSyncer { .. } | Self::O365 { .. })
    }

    pub fn supports_reload(&self) -> bool {
        matches!(self, Self::VDirSyncer { .. } | Self::O365 { .. })
    }

    pub fn path(&self, xdg: &BaseDirectories, name: &str) -> PathBuf {
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
    syncer: SyncerType,
    #[serde(rename = "calendar")]
    calendars: BTreeMap<String, CalendarSettings>,
}

impl CollectionSettings {
    pub fn new(syncer: SyncerType) -> Self {
        Self {
            syncer,
            calendars: BTreeMap::default(),
        }
    }

    pub fn path(&self, xdg: &BaseDirectories, name: &str) -> PathBuf {
        self.syncer.path(xdg, name)
    }

    pub fn email(&self) -> Option<&EmailAccount> {
        self.syncer.email()
    }

    pub fn build_organizer(&self) -> Option<CalOrganizer> {
        self.email()
            .map(|em| CalOrganizer::new_named(em.name().to_string(), em.address()))
    }

    pub fn syncer(&self) -> &SyncerType {
        &self.syncer
    }

    pub fn set_syncer(&mut self, syncer: SyncerType) {
        self.syncer = syncer;
    }

    pub fn calendars(&self) -> impl Iterator<Item = (&String, &CalendarSettings)> {
        self.calendars.iter().filter(|(_, c)| c.enabled())
    }

    pub fn all_calendars(&self) -> &BTreeMap<String, CalendarSettings> {
        &self.calendars
    }

    pub fn all_calendars_mut(&mut self) -> &mut BTreeMap<String, CalendarSettings> {
        &mut self.calendars
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct CalendarSettings {
    enabled: bool,
    folder: String,
    name: String,
    fgcolor: String,
    bgcolor: String,
    types: Vec<CalCompType>,
    alarms: CalendarAlarmType,
}

impl CalendarSettings {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn folder(&self) -> &String {
        &self.folder
    }

    pub fn set_folder(&mut self, folder: String) {
        self.folder = folder;
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn types(&self) -> &[CalCompType] {
        &self.types
    }

    pub fn set_types(&mut self, types: Vec<CalCompType>) {
        self.types = types;
    }

    pub fn fgcolor(&self) -> &String {
        &self.fgcolor
    }

    pub fn set_fgcolor(&mut self, color: String) {
        self.fgcolor = color;
    }

    pub fn bgcolor(&self) -> &String {
        &self.bgcolor
    }

    pub fn set_bgcolor(&mut self, color: String) {
        self.bgcolor = color;
    }

    pub fn alarms(&self) -> &CalendarAlarmType {
        &self.alarms
    }

    pub fn set_alarms(&mut self, alarms: CalendarAlarmType) {
        self.alarms = alarms;
    }
}
