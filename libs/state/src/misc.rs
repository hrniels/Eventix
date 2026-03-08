//! Helpers for persisted miscellaneous application state.
//!
//! This module provides the `Misc` type which encapsulates small pieces of persisted state
//! (locale choice, alarm/check timestamps, UI selections, disabled calendars and per-collection
//! tokens). It also provides helpers to load and persist that state using the project's XDG data
//! directory.

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

/// Holds small pieces of persisted application state.
///
/// Stores the configured locale type, timestamps used by alarm checks, the last selected calendar
/// per component type, lists of disabled calendars and calendar error markers, and stored
/// collection tokens.
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
    calendar_errors: HashSet<String>,
    #[serde(default)]
    collection_tokens: HashMap<String, String>,
}

impl Misc {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            locale_type: LocaleType::default(),
            last_alarm_check: chrono::Local::now().naive_utc(),
            last_calendar: HashMap::default(),
            disabled_calendars: Vec::default(),
            calendar_errors: HashSet::default(),
            collection_tokens: HashMap::default(),
        }
    }

    /// Returns the configured locale type used for formatting and translations.
    pub fn locale_type(&self) -> LocaleType {
        self.locale_type
    }

    /// Sets the configured locale type used for formatting and translations.
    pub fn set_locale_type(&mut self, ty: LocaleType) {
        self.locale_type = ty;
    }

    /// Returns the timestamp of the last performed alarm check.
    pub fn last_alarm_check(&self) -> NaiveDateTime {
        self.last_alarm_check
    }

    /// Sets the timestamp of the last performed alarm check.
    pub fn set_last_alarm_check(&mut self, datetime: NaiveDateTime) {
        self.last_alarm_check = datetime;
    }

    /// Returns the id of the last selected calendar for the given component type, if any.
    pub fn last_calendar(&self, ty: CalCompType) -> Option<&String> {
        self.last_calendar.get(&ty)
    }

    /// Sets the id of the last selected calendar for the specified component type.
    pub fn set_last_calendar(&mut self, ty: CalCompType, cal: String) {
        if let Some(e) = self.last_calendar.get_mut(&ty) {
            *e = cal;
        } else {
            self.last_calendar.insert(ty, cal);
        }
    }

    /// Returns true if the calendar identified by `id` is currently disabled.
    pub fn calendar_disabled(&self, id: &String) -> bool {
        self.disabled_calendars.contains(id)
    }

    /// Toggles the disabled state of the calendar identified by `id`.
    ///
    /// If the calendar was disabled it becomes enabled and vice versa.
    pub fn toggle_calendar(&mut self, id: &String) {
        if self.disabled_calendars.contains(id) {
            self.disabled_calendars.retain(|c| c != id);
        } else {
            self.disabled_calendars.push(id.to_string());
        }
    }

    /// Returns whether the calendar identified by `id` is currently marked as having an error.
    pub fn has_calendar_error(&self, id: &String) -> bool {
        self.calendar_errors.contains(id)
    }

    /// Marks or clears an error flag for the calendar identified by `id`.
    ///
    /// When `error` is true the calendar is marked as errored; when false the marker is removed.
    pub fn set_calendar_error(&mut self, id: &String, error: bool) {
        if self.has_calendar_error(id) && !error {
            self.calendar_errors.remove(id);
        } else if error {
            self.calendar_errors.insert(id.clone());
        }
    }

    /// Returns the stored authentication token for the collection `id`, if present.
    pub fn collection_token(&self, id: &String) -> Option<&String> {
        self.collection_tokens.get(id)
    }

    /// Stores an authentication token for the collection identified by `id`, replacing any
    /// previously stored token.
    pub fn set_collection_token(&mut self, id: &String, token: String) {
        *self.collection_tokens.entry(id.to_string()).or_default() = token;
    }

    /// Loads misc state from the configured XDG data file, or creates a new default instance if
    /// the file does not exist.
    ///
    /// Returns an error if reading or parsing the existing data file fails.
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

    /// Persists the misc state to its file on disk.
    ///
    /// Returns an error if serialization or file I/O fails.
    pub fn write_to_file(&self) -> anyhow::Result<()> {
        super::write_to_file(&self.path, self)
    }
}
