use eventix_ical::objects::{CalAlarm, CalCompType, CalOrganizer};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};
use xdg::BaseDirectories;

const FILENAME: &str = "settings.toml";

/// Top-level application settings, aggregating all collection and calendar configurations.
///
/// Loaded from and persisted to a TOML file in the XDG config directory. Collections are keyed by
/// name and each contains one or more calendar entries.
#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(skip)]
    path: PathBuf,
    #[serde(rename = "collection")]
    collections: BTreeMap<String, CollectionSettings>,
}

impl Settings {
    /// Creates a new empty `Settings` instance backed by the given file path.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            collections: BTreeMap::new(),
        }
    }

    /// Returns all collections as a map keyed by collection name.
    pub fn collections(&self) -> &BTreeMap<String, CollectionSettings> {
        &self.collections
    }

    /// Returns a mutable reference to all collections, keyed by collection name.
    pub fn collections_mut(&mut self) -> &mut BTreeMap<String, CollectionSettings> {
        &mut self.collections
    }

    /// Returns an iterator over all enabled calendars across all collections.
    ///
    /// Each item is a tuple of `(calendar_id, &CalendarSettings)`.
    pub fn calendars(&self) -> impl Iterator<Item = (&String, &CalendarSettings)> {
        self.collections
            .values()
            .flat_map(|col| col.calendars.iter().filter(|(_, c)| c.enabled()))
    }

    /// Returns the collection and calendar settings for the enabled calendar with the given `id`.
    ///
    /// Returns `None` if the calendar is not found or is disabled.
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

    /// Returns a map from calendar ID to formatted email address for all calendars in collections
    /// that have an associated email account.
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

    /// Loads settings from the XDG config file, or returns empty settings if no file exists.
    pub fn load_from_file(xdg: &BaseDirectories) -> anyhow::Result<Self> {
        match xdg.find_config_file(FILENAME) {
            Some(file) => {
                let mut settings: Settings = super::load_from_file(&file)?;
                settings.path = file;
                Ok(settings)
            }
            None => {
                let path = xdg.get_config_home().unwrap().join(FILENAME);
                Ok(Settings::new(path))
            }
        }
    }

    /// Persists the current settings to the associated TOML config file.
    pub fn write_to_file(&self) -> anyhow::Result<()> {
        super::write_to_file(&self.path, self)
    }
}

/// An email account with a display name and address.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailAccount {
    name: String,
    address: String,
}

impl EmailAccount {
    /// Creates a new `EmailAccount` with the given display name and address.
    pub fn new(name: String, address: String) -> Self {
        Self { name, address }
    }

    /// Returns the email address formatted as `"Name <address>"`.
    pub fn pretty_name(&self) -> String {
        format!("{} <{}>", self.name, self.address)
    }

    /// Returns the display name of the account.
    pub fn name(&self) -> &String {
        &self.name
    }

    /// Returns the email address in its original, unmodified form.
    pub fn org_address(&self) -> &String {
        &self.address
    }

    /// Returns the email address normalised to lowercase.
    pub fn address(&self) -> String {
        self.org_address().to_lowercase()
    }
}

/// Determines how alarms are sourced for a calendar.
///
/// `Calendar` uses the alarms embedded in the iCalendar data. `Personal` replaces them with
/// user-managed overrides, falling back to an optional default alarm when no override exists.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub enum CalendarAlarmType {
    #[default]
    Calendar,
    Personal {
        default: Option<CalAlarm>,
    },
}

/// The backend used to synchronise and provide calendar data for a collection.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// Returns the email account associated with this syncer, if any.
    ///
    /// `FileSystem` syncers have no email account and return `None`.
    pub fn email(&self) -> Option<&EmailAccount> {
        match self {
            Self::VDirSyncer { email, .. } => Some(email),
            Self::O365 { email, .. } => Some(email),
            _ => None,
        }
    }

    /// Returns whether this syncer supports calendar discovery.
    pub fn supports_discover(&self) -> bool {
        matches!(self, Self::VDirSyncer { .. } | Self::O365 { .. })
    }

    /// Returns whether this syncer supports reloading calendar data on demand.
    pub fn supports_reload(&self) -> bool {
        matches!(self, Self::VDirSyncer { .. } | Self::O365 { .. })
    }

    /// Returns the local filesystem path where calendar data for this syncer is stored.
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

/// Settings for a single collection, grouping a syncer backend with its associated calendars.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectionSettings {
    syncer: SyncerType,
    #[serde(rename = "calendar")]
    calendars: BTreeMap<String, CalendarSettings>,
}

impl CollectionSettings {
    /// Creates a new `CollectionSettings` with the given syncer and no calendars.
    pub fn new(syncer: SyncerType) -> Self {
        Self {
            syncer,
            calendars: BTreeMap::default(),
        }
    }

    /// Returns the local filesystem path where this collection's calendar data is stored.
    pub fn path(&self, xdg: &BaseDirectories, name: &str) -> PathBuf {
        self.syncer.path(xdg, name)
    }

    /// Returns the path to the sync log file for this collection.
    pub fn log_file(&self, xdg: &BaseDirectories, name: &str) -> PathBuf {
        let dir = xdg.get_data_file("vdirsyncer").unwrap();
        dir.join(format!("{}.log", name))
    }

    /// Returns the email account associated with this collection's syncer, if any.
    pub fn email(&self) -> Option<&EmailAccount> {
        self.syncer.email()
    }

    /// Returns a `CalOrganizer` built from the collection's email account, if one is configured.
    pub fn build_organizer(&self) -> Option<CalOrganizer> {
        self.email()
            .map(|em| CalOrganizer::new_named(em.name().to_string(), em.address()))
    }

    /// Returns the syncer configuration for this collection.
    pub fn syncer(&self) -> &SyncerType {
        &self.syncer
    }

    /// Sets the syncer configuration for this collection.
    pub fn set_syncer(&mut self, syncer: SyncerType) {
        self.syncer = syncer;
    }

    /// Returns an iterator over all enabled calendars in this collection.
    ///
    /// Each item is a tuple of `(calendar_id, &CalendarSettings)`.
    pub fn calendars(&self) -> impl Iterator<Item = (&String, &CalendarSettings)> {
        self.calendars.iter().filter(|(_, c)| c.enabled())
    }

    /// Returns all calendars in this collection, including disabled ones.
    pub fn all_calendars(&self) -> &BTreeMap<String, CalendarSettings> {
        &self.calendars
    }

    /// Returns a mutable reference to all calendars in this collection, including disabled ones.
    pub fn all_calendars_mut(&mut self) -> &mut BTreeMap<String, CalendarSettings> {
        &mut self.calendars
    }
}

/// Settings for a single calendar entry within a collection.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
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
    /// Returns whether this calendar is enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Sets whether this calendar is enabled.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns the folder name used to locate this calendar's data on disk.
    pub fn folder(&self) -> &String {
        &self.folder
    }

    /// Sets the folder name used to locate this calendar's data on disk.
    pub fn set_folder(&mut self, folder: String) {
        self.folder = folder;
    }

    /// Returns the display name of this calendar.
    pub fn name(&self) -> &String {
        &self.name
    }

    /// Sets the display name of this calendar.
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    /// Returns the component types this calendar contains (e.g. events, tasks).
    pub fn types(&self) -> &[CalCompType] {
        &self.types
    }

    /// Sets the component types this calendar contains.
    pub fn set_types(&mut self, types: Vec<CalCompType>) {
        self.types = types;
    }

    /// Returns the foreground colour for this calendar's display.
    pub fn fgcolor(&self) -> &String {
        &self.fgcolor
    }

    /// Sets the foreground colour for this calendar's display.
    pub fn set_fgcolor(&mut self, color: String) {
        self.fgcolor = color;
    }

    /// Returns the background colour for this calendar's display.
    pub fn bgcolor(&self) -> &String {
        &self.bgcolor
    }

    /// Sets the background colour for this calendar's display.
    pub fn set_bgcolor(&mut self, color: String) {
        self.bgcolor = color;
    }

    /// Returns the alarm type configuration for this calendar.
    pub fn alarms(&self) -> &CalendarAlarmType {
        &self.alarms
    }

    /// Sets the alarm type configuration for this calendar.
    pub fn set_alarms(&mut self, alarms: CalendarAlarmType) {
        self.alarms = alarms;
    }
}
