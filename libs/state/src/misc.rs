// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Helpers for persisted miscellaneous application state.
//!
//! This module provides the `Misc` type which encapsulates small pieces of persisted state
//! (alarm/check timestamps, UI selections, disabled calendars and per-collection
//! tokens). It also provides helpers to load and persist that state using the project's XDG data
//! directory.

use chrono::NaiveDateTime;
use eventix_ical::objects::CalCompType;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use xdg::BaseDirectories;

const FILENAME: &str = "misc.toml";

/// Holds small pieces of persisted application state.
///
/// Stores timestamps used by alarm checks, the last selected calendar
/// per component type, disabled calendars, collection synchronization errors and stored tokens.
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Misc {
    #[serde(default)]
    version: u32,
    #[serde(skip)]
    path: PathBuf,
    #[serde(default)]
    last_alarm_check: NaiveDateTime,
    #[serde(default)]
    last_calendar: HashMap<CalCompType, String>,
    #[serde(default)]
    disabled_calendars: Vec<String>,
    #[serde(default)]
    collection_errors: HashMap<String, String>,
    #[serde(default)]
    collection_tokens: HashMap<String, String>,
}

impl Misc {
    /// Creates a new `Misc` with default values.
    ///
    /// All fields are initialised to their defaults, except `last_alarm_check` which is
    /// set to the current UTC time so that alarms that fell due before the process started
    /// are not immediately re-fired.
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            version: crate::CURRENT_VERSION,
            path,
            last_alarm_check: chrono::Local::now().naive_utc(),
            last_calendar: HashMap::default(),
            disabled_calendars: Vec::default(),
            collection_errors: HashMap::default(),
            collection_tokens: HashMap::default(),
        }
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

    /// Returns the last synchronization error message for the collection, if present.
    ///
    /// An empty message indicates a migrated failure whose original message was not stored.
    pub fn collection_error(&self, id: &str) -> Option<&str> {
        self.collection_errors.get(id).map(String::as_str)
    }

    /// Stores the last synchronization error for the collection, or clears it on success.
    pub fn set_collection_error(&mut self, id: &str, error: Option<&str>) {
        if let Some(error) = error {
            self.collection_errors
                .insert(id.to_owned(), error.to_owned());
        } else {
            self.collection_errors.remove(id);
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
                crate::migration::migrate_misc_if_needed(xdg)?;
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

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use eventix_ical::objects::CalCompType;
    use std::os::unix::fs::PermissionsExt;

    use super::Misc;

    fn make_misc() -> Misc {
        Misc::new(std::path::PathBuf::default())
    }

    #[test]
    fn last_alarm_check_get_and_set() {
        let mut m = make_misc();
        let dt = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        m.set_last_alarm_check(dt);
        assert_eq!(m.last_alarm_check(), dt);
    }

    #[test]
    fn last_calendar_get_and_set() {
        let mut m = make_misc();
        assert_eq!(m.last_calendar(CalCompType::Event), None);
        m.set_last_calendar(CalCompType::Event, "cal1".to_string());
        assert_eq!(
            m.last_calendar(CalCompType::Event),
            Some(&"cal1".to_string())
        );
        // update existing entry
        m.set_last_calendar(CalCompType::Event, "cal2".to_string());
        assert_eq!(
            m.last_calendar(CalCompType::Event),
            Some(&"cal2".to_string())
        );
    }

    #[test]
    fn toggle_calendar_enables_and_disables() {
        let mut m = make_misc();
        let id = "my-cal".to_string();
        assert!(!m.calendar_disabled(&id));
        m.toggle_calendar(&id);
        assert!(m.calendar_disabled(&id));
        m.toggle_calendar(&id);
        assert!(!m.calendar_disabled(&id));
    }

    #[test]
    fn collection_token_get_and_set() {
        let mut m = make_misc();
        let id = "col".to_string();
        assert_eq!(m.collection_token(&id), None);
        m.set_collection_token(&id, "tok1".to_string());
        assert_eq!(m.collection_token(&id), Some(&"tok1".to_string()));
        // replace existing token
        m.set_collection_token(&id, "tok2".to_string());
        assert_eq!(m.collection_token(&id), Some(&"tok2".to_string()));
    }

    #[test]
    fn collection_error_set_replace_and_clear() {
        let mut misc = make_misc();
        assert_eq!(misc.collection_error("work"), None);
        misc.set_collection_error("work", Some("Connection refused"));
        assert_eq!(misc.collection_error("work"), Some("Connection refused"));
        misc.set_collection_error("work", Some("Server unavailable"));
        assert_eq!(misc.collection_error("work"), Some("Server unavailable"));
        misc.set_collection_error("work", None);
        misc.set_collection_error("work", None);
        assert_eq!(misc.collection_error("work"), None);
    }

    #[test]
    fn write_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("misc.toml");
        std::fs::write(&path, "old content").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut m = Misc::new(path.clone());
        m.set_collection_token(&"col".to_string(), "tok".to_string());
        m.write_to_file().unwrap();

        let loaded: Misc = crate::load_from_file(&path).unwrap();
        assert_eq!(
            loaded.collection_token(&"col".to_string()),
            Some(&"tok".to_string())
        );
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
