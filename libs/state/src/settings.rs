// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
        /// Shell command and arguments used to retrieve the password at runtime, if any.
        password_cmd: Option<Vec<String>>,
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
        /// Shell command and arguments used to retrieve the OAuth password/token at runtime.
        password_cmd: Vec<String>,
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
            password_cmd: None,
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
