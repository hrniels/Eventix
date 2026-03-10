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
    /// Creates a new `Misc` with default values.
    ///
    /// All fields are initialised to their defaults, except `last_alarm_check` which is
    /// set to the current UTC time so that alarms that fell due before the process started
    /// are not immediately re-fired.
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

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use eventix_ical::objects::CalCompType;
    use eventix_locale::LocaleType;

    use super::Misc;

    fn make_misc() -> Misc {
        Misc::new(std::path::PathBuf::default())
    }

    #[test]
    fn locale_type_get_and_set() {
        let mut m = make_misc();
        assert_eq!(m.locale_type(), LocaleType::English);
        m.set_locale_type(LocaleType::German);
        assert_eq!(m.locale_type(), LocaleType::German);
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
    fn calendar_error_set_and_clear() {
        let mut m = make_misc();
        let id = "cal".to_string();
        assert!(!m.has_calendar_error(&id));
        m.set_calendar_error(&id, true);
        assert!(m.has_calendar_error(&id));
        // setting true again is a no-op
        m.set_calendar_error(&id, true);
        assert!(m.has_calendar_error(&id));
        m.set_calendar_error(&id, false);
        assert!(!m.has_calendar_error(&id));
        // clearing an absent entry is a no-op
        m.set_calendar_error(&id, false);
        assert!(!m.has_calendar_error(&id));
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
    fn write_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("misc.toml");
        let mut m = Misc::new(path.clone());
        m.set_locale_type(LocaleType::German);
        m.set_collection_token(&"col".to_string(), "tok".to_string());
        m.write_to_file().unwrap();

        let loaded: Misc = crate::load_from_file(&path).unwrap();
        assert_eq!(loaded.locale_type(), LocaleType::German);
        assert_eq!(
            loaded.collection_token(&"col".to_string()),
            Some(&"tok".to_string())
        );
    }
}
