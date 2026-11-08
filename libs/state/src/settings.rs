// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::objects::{CalAlarm, CalCompType, CalOrganizer};
use eventix_locale::LocaleType;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};
use xdg::BaseDirectories;

const FILENAME: &str = "settings.toml";

/// User preferences shared by the application frontends.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Language used for translations and formatting.
    pub locale_type: LocaleType,
    /// Minutes between automatic full synchronizations (1–1,440).
    pub sync_interval_minutes: u32,
    /// Seconds to wait after the last edit before uploading (0–86,400).
    pub post_delay_seconds: u32,
}

/// Reports unsupported preference values.
#[derive(Debug, thiserror::Error)]
pub enum PreferencesError {
    #[error("Invalid sync interval")]
    SyncInterval,
    #[error("Invalid post delay")]
    PostDelay,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            locale_type: LocaleType::default(),
            sync_interval_minutes: 15,
            post_delay_seconds: 5,
        }
    }
}

impl Preferences {
    /// Checks that synchronization timings are within the supported ranges.
    pub fn validate(&self) -> Result<(), PreferencesError> {
        if !(1..=1440).contains(&self.sync_interval_minutes) {
            return Err(PreferencesError::SyncInterval);
        }
        if self.post_delay_seconds > 86_400 {
            return Err(PreferencesError::PostDelay);
        }
        Ok(())
    }
}

/// Top-level application settings, including preferences and calendar configurations.
///
/// Loaded from and persisted to a TOML file in the XDG config directory. Collections are keyed by
/// name and each contains one or more calendar entries. User preferences are stored in the
/// `preferences` table.
#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    version: u32,
    #[serde(skip)]
    path: PathBuf,
    #[serde(default)]
    preferences: Preferences,
    #[serde(rename = "collection")]
    collections: BTreeMap<String, CollectionSettings>,
}

impl Settings {
    /// Creates a new empty `Settings` instance backed by the given file path.
    pub fn new(path: PathBuf) -> Self {
        Self {
            version: crate::CURRENT_VERSION,
            path,
            preferences: Preferences::default(),
            collections: BTreeMap::new(),
        }
    }

    /// Returns the current user preferences.
    pub fn preferences(&self) -> Preferences {
        self.preferences
    }

    /// Replaces the user preferences after validating their timing ranges.
    pub fn set_preferences(&mut self, preferences: Preferences) -> Result<(), PreferencesError> {
        preferences.validate()?;
        self.preferences = preferences;
        Ok(())
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
    /// Each item is a tuple of `(calendar_id, &CalendarSettings, collection_id)`.
    pub fn calendars_with_col(
        &self,
    ) -> impl Iterator<Item = (&String, &CalendarSettings, &String)> {
        self.collections.iter().flat_map(|(col_id, col)| {
            col.calendars
                .iter()
                .filter(|(_, c)| c.enabled())
                .map(move |(cal_id, c)| (cal_id, c, col_id))
        })
    }

    /// Returns an iterator over all enabled calendars across all collections.
    ///
    /// Each item is a tuple of `(calendar_id, &CalendarSettings)`.
    pub fn calendars(&self) -> impl Iterator<Item = (&String, &CalendarSettings)> {
        self.calendars_with_col().map(|(id, c, _)| (id, c))
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
        let (mut settings, has_preferences) = match xdg.find_config_file(FILENAME) {
            Some(file) => {
                let document: toml::Value = super::load_from_file(&file)?;
                let has_preferences = document.get("preferences").is_some();
                let mut settings: Settings = document.try_into()?;
                settings.path = file;
                (settings, has_preferences)
            }
            None => {
                let path = xdg.get_config_home().unwrap().join(FILENAME);
                (Settings::new(path), false)
            }
        };
        if !has_preferences {
            settings.import_legacy_preferences(xdg)?;
        }
        settings.preferences.validate()?;
        Ok(settings)
    }

    /// Imports old preferences once; an existing preferences table always takes precedence.
    fn import_legacy_preferences(&mut self, xdg: &BaseDirectories) -> anyhow::Result<()> {
        let Some(path) = xdg.find_data_file("misc.toml") else {
            return Ok(());
        };
        crate::migration::migrate_misc_if_needed(xdg)?;
        let document: toml::Value = super::load_from_file(&path)?;
        if ["locale_type", "sync_interval_minutes", "post_delay_seconds"]
            .iter()
            .any(|key| document.get(key).is_some())
        {
            self.set_preferences(document.try_into()?)?;
            self.path = xdg.place_config_file(FILENAME)?;
            self.write_to_file()?;
        }
        Ok(())
    }

    /// Persists the current settings to the associated TOML config file.
    pub fn write_to_file(&self) -> anyhow::Result<()> {
        super::write_to_file(&self.path, self)
    }
}

/// An email account with a display name and address.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CalendarAlarmType {
    /// Uses the alarms embedded in the iCalendar data as-is.
    #[default]
    Calendar,
    /// Replaces embedded alarms with user-managed personal overrides.
    ///
    /// When no per-event override exists, `default` is used as a fallback alarm.
    /// If `default` is `None` and no override exists, no alarm fires.
    Personal {
        /// The fallback alarm applied when no per-event override exists.
        default: Option<CalAlarm>,
    },
}

/// One bound of a sync time span.
///
/// Used to configure how far back or forward in time vdirsyncer should request calendar items
/// from the CalDAV server.
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SyncTimeBound {
    /// Sync up to N years before/after the current time.
    Years(u32),
    /// No bound in this direction; sync everything on this side.
    #[default]
    Infinite,
}

/// The time range to request from the CalDAV server during synchronisation.
///
/// When at least one bound is `Years`, both `start_date` and `end_date` are emitted in the
/// generated vdirsyncer configuration. An `Infinite` bound uses `timedelta(days=365*100)` as a
/// sentinel to express "effectively unbounded" on that side, since vdirsyncer requires both
/// `start_date` and `end_date` to be present whenever either is specified.
///
/// When both bounds are `Infinite` (the default), no date filter is emitted and vdirsyncer
/// synchronises the full calendar.
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncTimeSpan {
    /// Lower bound of the sync window relative to now.
    #[serde(default)]
    pub start: SyncTimeBound,
    /// Upper bound of the sync window relative to now.
    #[serde(default)]
    pub end: SyncTimeBound,
}

impl SyncTimeSpan {
    /// Returns `true` when at least one bound is finite and date filter lines should be emitted.
    pub fn needs_date_filter(&self) -> bool {
        matches!(self.start, SyncTimeBound::Years(_)) || matches!(self.end, SyncTimeBound::Years(_))
    }

    /// Returns the vdirsyncer Python expression for `start_date`.
    ///
    /// When the start bound is `Infinite`, a sentinel expression equivalent to approximately
    /// 100 years in the past is returned so that the vdirsyncer requirement of providing both
    /// dates together is satisfied.
    pub fn start_expr(&self) -> String {
        match self.start {
            SyncTimeBound::Years(n) => format!("datetime.now() - timedelta(days=365*{})", n),
            SyncTimeBound::Infinite => "datetime.now() - timedelta(days=365*100)".to_string(),
        }
    }

    /// Returns the vdirsyncer Python expression for `end_date`.
    ///
    /// When the end bound is `Infinite`, a sentinel expression equivalent to approximately
    /// 100 years in the future is returned for the same reason as `start_expr`.
    pub fn end_expr(&self) -> String {
        match self.end {
            SyncTimeBound::Years(n) => format!("datetime.now() + timedelta(days=365*{})", n),
            SyncTimeBound::Infinite => "datetime.now() + timedelta(days=365*100)".to_string(),
        }
    }
}

/// A persisted representation of an encrypted password.
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncryptedPassword {
    /// The random nonce used for encryption.
    pub nonce: String,
    /// The encrypted password as base64-encoded ciphertext.
    pub ciphertext: String,
}

/// The backend used to synchronise and provide calendar data for a collection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SyncerType {
    /// Reads calendar data directly from a local filesystem path; no remote sync.
    FileSystem {
        /// Absolute path to the directory containing the calendar collection.
        path: String,
    },
    /// Synchronises via the `vdirsyncer` tool against a CalDAV server.
    VDirSyncer {
        /// Email account associated with this collection.
        email: EmailAccount,
        /// CalDAV server URL.
        url: String,
        /// When `true`, the remote calendar is treated as read-only and local changes are not pushed.
        read_only: bool,
        /// Optional username for authentication; if absent, no credentials are sent.
        username: Option<String>,
        /// The encrypted password, if any.
        #[serde(default)]
        password: Option<EncryptedPassword>,
        /// The time range to synchronise from the CalDAV server.
        ///
        /// Defaults to both bounds being `Infinite`, which synchronises everything.
        #[serde(default)]
        time_span: SyncTimeSpan,
    },
    /// Synchronises a Microsoft 365 account via DavMail as a local CalDAV gateway.
    O365 {
        /// Email account associated with this collection.
        email: EmailAccount,
        /// When `true`, the remote calendar is treated as read-only and local changes are not pushed.
        read_only: bool,
        /// The encrypted password.
        password: EncryptedPassword,
        /// The time range to synchronise from the CalDAV server.
        ///
        /// Defaults to both bounds being `Infinite`, which synchronises everything.
        #[serde(default)]
        time_span: SyncTimeSpan,
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

    /// Returns the encrypted password for this syncer, if any.
    pub fn password(&self) -> Option<&EncryptedPassword> {
        match self {
            Self::VDirSyncer { password, .. } => password.as_ref(),
            Self::O365 { password, .. } => Some(password),
            _ => None,
        }
    }

    /// Returns the configured time span for this syncer, if any.
    ///
    /// `FileSystem` syncers do not use a time span and return `None`.
    pub fn time_span(&self) -> Option<&SyncTimeSpan> {
        match self {
            Self::VDirSyncer { time_span, .. } | Self::O365 { time_span, .. } => Some(time_span),
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

    /// Returns whether this syncer supports the creation and deletion of calendars
    pub fn supports_mkrm(&self) -> bool {
        matches!(self, Self::VDirSyncer { .. } | Self::FileSystem { .. })
    }

    /// Returns whether this syncer is configured as read-only.
    pub fn is_read_only(&self) -> bool {
        match self {
            Self::VDirSyncer { read_only, .. } | Self::O365 { read_only, .. } => *read_only,
            Self::FileSystem { .. } => false,
        }
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

    /// Returns whether this collection is configured as read-only.
    ///
    /// This respects debug mode: in debug mode, everything is writable. In release mode, this
    /// returns the syncer's read-only state.
    pub fn is_read_only(&self) -> bool {
        if !cfg!(test) && cfg!(debug_assertions) {
            return false;
        }
        self.is_syncer_read_only()
    }

    /// Returns whether the syncer itself is configured as read-only.
    pub fn is_syncer_read_only(&self) -> bool {
        self.syncer.is_read_only()
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
#[derive(Clone, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use eventix_ical::objects::CalCompType;

    use super::{
        CalendarAlarmType, CalendarSettings, CollectionSettings, EmailAccount, Settings,
        SyncTimeBound, SyncTimeSpan, SyncerType,
    };

    #[test]
    fn preferences_default_for_existing_files_and_validate_updates() {
        let mut settings: Settings = toml::from_str("[collection]").unwrap();
        let mut preferences = settings.preferences();
        assert_eq!(preferences, super::Preferences::default());
        for interval in [0, 1441, u32::MAX] {
            preferences.sync_interval_minutes = interval;
            assert!(settings.set_preferences(preferences).is_err());
            assert_eq!(settings.preferences().sync_interval_minutes, 15);
        }
        preferences.sync_interval_minutes = 1440;
        for delay in [86_401, u32::MAX] {
            preferences.post_delay_seconds = delay;
            assert!(settings.set_preferences(preferences).is_err());
            assert_eq!(settings.preferences(), super::Preferences::default());
        }
        preferences.post_delay_seconds = 86_400;
        settings.set_preferences(preferences).unwrap();
        assert_eq!(settings.preferences(), preferences);
        preferences.post_delay_seconds = 0;
        preferences.locale_type = eventix_locale::LocaleType::German;
        settings.set_preferences(preferences).unwrap();
        let serialized = toml::to_string(&settings).unwrap();
        assert!(serialized.contains("[preferences]"));
        let loaded: Settings = toml::from_str(&serialized).unwrap();
        assert_eq!(loaded.preferences(), preferences);
    }

    #[test]
    fn preferences_import_legacy_values_once_and_keep_misc_state_separate() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let config = tmp.path().join("config");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        let xdg = crate::with_test_xdg(&data, &config);
        std::fs::write(config.join("settings.toml"), "version = 3\n[collection]\n").unwrap();
        std::fs::write(data.join("misc.toml"),
            "version = 3\nlocale_type = \"German\"\nsync_interval_minutes = 30\npost_delay_seconds = 12\ndisabled_calendars = [\"test\"]\n").unwrap();
        let settings = Settings::load_from_file(&xdg).unwrap();
        assert_eq!(
            settings.preferences(),
            super::Preferences {
                locale_type: eventix_locale::LocaleType::German,
                sync_interval_minutes: 30,
                post_delay_seconds: 12,
            }
        );
        assert!(settings.collections().is_empty());
        let mut misc = crate::Misc::load_from_file(&xdg).unwrap();
        assert!(misc.calendar_disabled(&"test".into()));
        misc.toggle_calendar(&"test".into());
        misc.write_to_file().unwrap();
        let contents = std::fs::read_to_string(data.join("misc.toml")).unwrap();
        assert!(!contents.contains("locale_type"));
        assert!(!contents.contains("sync_interval_minutes"));
        assert!(!contents.contains("post_delay_seconds"));
        // An older misc file cannot override preferences already stored in settings.
        std::fs::write(
            data.join("misc.toml"),
            "version = 3\nlocale_type = \"English\"\n",
        )
        .unwrap();
        assert_eq!(
            Settings::load_from_file(&xdg).unwrap().preferences(),
            settings.preferences()
        );
    }

    #[test]
    fn preferences_import_old_language_without_timing_values() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = crate::with_test_xdg(tmp.path(), tmp.path());
        std::fs::write(
            tmp.path().join("misc.toml"),
            "version = 3\nlocale_type = \"German\"\n",
        )
        .unwrap();
        let preferences = Settings::load_from_file(&xdg).unwrap().preferences();
        assert_eq!(preferences.locale_type, eventix_locale::LocaleType::German);
        assert_eq!(preferences.sync_interval_minutes, 15);
        assert_eq!(preferences.post_delay_seconds, 5);
    }

    // --- helpers ---

    fn make_email() -> EmailAccount {
        EmailAccount::new("Alice Example".to_string(), "Alice@Example.COM".to_string())
    }

    fn make_filesystem_syncer() -> SyncerType {
        SyncerType::FileSystem {
            path: "/data/calendars".to_string(),
        }
    }

    fn make_vdirsyncer() -> SyncerType {
        SyncerType::VDirSyncer {
            email: make_email(),
            url: "https://dav.example.com".to_string(),
            read_only: false,
            username: Some("alice".to_string()),
            password: None,
            time_span: Default::default(),
        }
    }

    /// Creates a simple `CalendarSettings` with the given enabled state, folder, and name.
    fn make_cal_settings(enabled: bool, folder: &str, name: &str) -> CalendarSettings {
        let mut cal = CalendarSettings::default();
        cal.set_enabled(enabled);
        cal.set_folder(folder.to_string());
        cal.set_name(name.to_string());
        cal
    }

    // --- SyncTimeSpan ---

    #[test]
    fn sync_time_span_needs_date_filter() {
        // Both infinite → no filter needed.
        let both_inf = SyncTimeSpan::default();
        assert!(!both_inf.needs_date_filter());

        // Start finite, end infinite → filter needed.
        let start_only = SyncTimeSpan {
            start: SyncTimeBound::Years(2),
            end: SyncTimeBound::Infinite,
        };
        assert!(start_only.needs_date_filter());

        // Start infinite, end finite → filter needed.
        let end_only = SyncTimeSpan {
            start: SyncTimeBound::Infinite,
            end: SyncTimeBound::Years(1),
        };
        assert!(end_only.needs_date_filter());

        // Both finite → filter needed.
        let both_finite = SyncTimeSpan {
            start: SyncTimeBound::Years(2),
            end: SyncTimeBound::Years(1),
        };
        assert!(both_finite.needs_date_filter());
    }

    #[test]
    fn sync_time_span_expressions() {
        // Finite start bound.
        let span = SyncTimeSpan {
            start: SyncTimeBound::Years(3),
            end: SyncTimeBound::Years(1),
        };
        assert_eq!(span.start_expr(), "datetime.now() - timedelta(days=365*3)");
        assert_eq!(span.end_expr(), "datetime.now() + timedelta(days=365*1)");

        // Infinite bounds fall back to sentinel values.
        let inf = SyncTimeSpan::default();
        assert!(inf.start_expr().contains("100"));
        assert!(inf.end_expr().contains("100"));
    }

    // --- EmailAccount ---

    #[test]
    fn email_account_accessors() {
        let email = make_email();

        assert_eq!(email.name(), "Alice Example");
        // org_address returns the original, unmodified casing.
        assert_eq!(email.org_address(), "Alice@Example.COM");
        // address() returns the lowercased form.
        assert_eq!(email.address(), "alice@example.com");
        // pretty_name formats as "Name <address>".
        assert_eq!(email.pretty_name(), "Alice Example <Alice@Example.COM>");
    }

    // --- CalendarSettings ---

    #[test]
    fn calendar_settings_accessors() {
        let mut cal = CalendarSettings::default();

        // Defaults
        assert!(!cal.enabled());
        assert_eq!(cal.folder(), "");
        assert_eq!(cal.name(), "");
        assert_eq!(cal.fgcolor(), "");
        assert_eq!(cal.bgcolor(), "");
        assert!(cal.types().is_empty());
        assert!(matches!(cal.alarms(), CalendarAlarmType::Calendar));

        // Setters
        cal.set_enabled(true);
        assert!(cal.enabled());

        cal.set_folder("home".to_string());
        assert_eq!(cal.folder(), "home");

        cal.set_name("Personal".to_string());
        assert_eq!(cal.name(), "Personal");

        cal.set_fgcolor("#ffffff".to_string());
        assert_eq!(cal.fgcolor(), "#ffffff");

        cal.set_bgcolor("#000000".to_string());
        assert_eq!(cal.bgcolor(), "#000000");

        let types = vec![CalCompType::Event, CalCompType::Todo];
        cal.set_types(types.clone());
        assert_eq!(cal.types(), types.as_slice());

        cal.set_alarms(CalendarAlarmType::Personal { default: None });
        assert!(matches!(
            cal.alarms(),
            CalendarAlarmType::Personal { default: None }
        ));
    }

    // --- CollectionSettings ---

    #[test]
    fn collection_settings_calendars_filter() {
        let mut col = CollectionSettings::new(make_filesystem_syncer());

        col.all_calendars_mut().insert(
            "enabled-cal".to_string(),
            make_cal_settings(true, "enabled", "Enabled"),
        );
        col.all_calendars_mut().insert(
            "disabled-cal".to_string(),
            make_cal_settings(false, "disabled", "Disabled"),
        );

        // calendars() only yields enabled entries.
        let enabled: Vec<_> = col.calendars().map(|(id, _)| id.clone()).collect();
        assert_eq!(enabled, vec!["enabled-cal"]);

        // all_calendars() yields both.
        assert_eq!(col.all_calendars().len(), 2);
    }

    #[test]
    fn collection_settings_email_and_organizer() {
        // A FileSystem syncer has no email; build_organizer returns None.
        let col_fs = CollectionSettings::new(make_filesystem_syncer());
        assert!(col_fs.email().is_none());
        assert!(col_fs.build_organizer().is_none());

        // A remote syncer has an email; build_organizer returns Some.
        let col_remote = CollectionSettings::new(make_vdirsyncer());
        let email = col_remote
            .email()
            .expect("VDirSyncer collection must have email");
        assert_eq!(email.name(), "Alice Example");
        let organizer = col_remote
            .build_organizer()
            .expect("organizer must be present");
        // The organizer encodes the lowercased address.
        assert!(organizer.address().contains("alice@example.com"));
    }

    // --- Settings ---

    #[test]
    fn settings_calendars_iterator() {
        let mut settings = Settings::new("/tmp/settings.toml".into());

        let mut col = CollectionSettings::new(make_filesystem_syncer());
        col.all_calendars_mut()
            .insert("cal-a".to_string(), make_cal_settings(true, "a", "A"));
        col.all_calendars_mut()
            .insert("cal-b".to_string(), make_cal_settings(false, "b", "B"));
        settings.collections_mut().insert("col1".to_string(), col);

        // calendars() only returns enabled calendars across all collections.
        let ids: Vec<_> = settings.calendars().map(|(id, _)| id.clone()).collect();
        assert_eq!(ids, vec!["cal-a"]);
    }

    #[test]
    fn settings_calendar_lookup() {
        let mut settings = Settings::new("/tmp/settings.toml".into());

        let mut col = CollectionSettings::new(make_filesystem_syncer());
        col.all_calendars_mut()
            .insert("cal-on".to_string(), make_cal_settings(true, "on", "On"));
        col.all_calendars_mut().insert(
            "cal-off".to_string(),
            make_cal_settings(false, "off", "Off"),
        );
        settings.collections_mut().insert("col1".to_string(), col);

        // Found and enabled — returns Some.
        let (_, cal) = settings
            .calendar(&"cal-on".to_string())
            .expect("must find enabled calendar");
        assert_eq!(cal.name(), "On");

        // Found but disabled — returns None.
        assert!(settings.calendar(&"cal-off".to_string()).is_none());

        // Not found — returns None.
        assert!(settings.calendar(&"missing".to_string()).is_none());
    }

    #[test]
    fn settings_emails() {
        let mut settings = Settings::new("/tmp/settings.toml".into());

        // Collection with no email account — no entries added.
        let mut col_fs = CollectionSettings::new(make_filesystem_syncer());
        col_fs
            .all_calendars_mut()
            .insert("cal-fs".to_string(), make_cal_settings(true, "fs", "FS"));
        settings
            .collections_mut()
            .insert("fs-col".to_string(), col_fs);

        // Collection with an email account — all calendar IDs mapped to pretty_name.
        let mut col_remote = CollectionSettings::new(make_vdirsyncer());
        col_remote.all_calendars_mut().insert(
            "cal-remote".to_string(),
            make_cal_settings(true, "remote", "Remote"),
        );
        settings
            .collections_mut()
            .insert("remote-col".to_string(), col_remote);

        let emails = settings.emails();
        assert!(
            !emails.contains_key("cal-fs"),
            "FileSystem collection must not appear in emails"
        );
        assert_eq!(
            emails
                .get("cal-remote")
                .expect("remote calendar must have email"),
            "Alice Example <Alice@Example.COM>"
        );
    }
}
