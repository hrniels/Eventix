mod misc;
mod persalarms;
mod settings;
mod sync;

/// Utility helpers exposed to other crates and tests.
pub mod util;

use anyhow::{Context, anyhow};
use chrono::NaiveDateTime;
use eventix_ical::{
    col::{CalDir, CalStore},
    objects::{EventLike, UpdatableEventLike},
};
use eventix_locale::Locale;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Mutex;
use tracing::debug;
use xdg::BaseDirectories;

pub use persalarms::{PersonalAlarms, PersonalCalendarAlarms};
pub use settings::{
    CalendarAlarmType, CalendarSettings, CollectionSettings, EmailAccount, Settings, SyncerType,
};
pub use sync::{SyncColResult, SyncResult, Syncer, log_file};

/// Shared, async-safe handle to the global application state.
///
/// This type is an `Arc<Mutex<State>>` so it can be cloned and held across asynchronous tasks.
/// Acquire access with `lock().await` to mutate or read the inner `State`.
pub type EventixState = Arc<Mutex<State>>;

/// In-memory application state.
///
/// Holds runtime representations of persisted data such as calendars, settings, personal alarms
/// and locale. This struct should be wrapped in `EventixState` (an `Arc<Mutex<_>>`) when shared
/// across async tasks.
pub struct State {
    xdg: Arc<BaseDirectories>,
    store: CalStore,
    personal_alarms: PersonalAlarms,
    settings: settings::Settings,
    misc: misc::Misc,
    locale: Arc<dyn Locale + Send + Sync>,
    last_reload: NaiveDateTime,
}

impl State {
    /// Creates a new in-memory application `State` by loading persisted data from the provided XDG
    /// base directories.
    ///
    /// Loads settings, personal alarms, misc state, and constructs the calendar store from all
    /// configured collections. Returns an error if any of the underlying loads fail.
    pub fn new(xdg: Arc<BaseDirectories>) -> anyhow::Result<Self> {
        let settings = settings::Settings::load_from_file(&xdg).context("load settings")?;

        let personal_alarms = PersonalAlarms::new_from_dir(&xdg).context("load personal alarms")?;

        let misc = misc::Misc::load_from_file(&xdg).context("load misc state")?;
        let locale = eventix_locale::new(&xdg, misc.locale_type())?;

        let mut store = CalStore::default();
        for (col_id, col) in settings.collections().iter() {
            for (cal_id, cal) in col.calendars() {
                let dir = Self::load_calendar(&xdg, col_id, col, cal_id, cal)?;
                store.add(dir);
            }
        }

        Ok(Self {
            xdg,
            locale,
            settings,
            personal_alarms,
            store,
            misc,
            last_reload: chrono::Utc::now().naive_utc(),
        })
    }

    /// Creates a minimal in-memory `State` for use in unit tests.
    ///
    /// Accepts an already-built `CalStore` and `Misc` so tests can exercise logic that requires a
    /// fully wired `State` without touching the filesystem. Uses a default `BaseDirectories`
    /// snapshot (from the current environment at call time).
    #[cfg(test)]
    pub(crate) fn new_for_test(store: CalStore, misc: misc::Misc) -> Self {
        Self {
            xdg: Arc::new(xdg::BaseDirectories::with_prefix("")),
            store,
            personal_alarms: PersonalAlarms::default(),
            settings: settings::Settings::new(PathBuf::default()),
            misc,
            locale: eventix_locale::default(),
            last_reload: chrono::Utc::now().naive_utc(),
        }
    }

    /// Creates a minimal in-memory `State` for use in unit tests, with an explicit XDG base.
    ///
    /// Like `new_for_test` but accepts a pre-built `BaseDirectories` so that tests which mutate
    /// XDG env vars can pass in their own snapshot without relying on the ambient environment at
    /// construction time.
    #[cfg(test)]
    pub(crate) fn new_for_test_with_xdg(
        xdg: xdg::BaseDirectories,
        store: CalStore,
        misc: misc::Misc,
    ) -> Self {
        Self {
            xdg: Arc::new(xdg),
            store,
            personal_alarms: PersonalAlarms::default(),
            settings: settings::Settings::new(PathBuf::default()),
            misc,
            locale: eventix_locale::default(),
            last_reload: chrono::Utc::now().naive_utc(),
        }
    }

    /// Reloads the `Locale` implementation from persisted misc state.
    ///
    /// Call this after the user changes language/locale preferences so that formatting and
    /// translations are updated without restarting the application. Returns an error if creating
    /// the locale fails.
    pub fn reload_locale(&mut self) -> anyhow::Result<()> {
        self.locale = eventix_locale::new(&self.xdg, self.misc.locale_type())?;
        Ok(())
    }

    /// Refreshes the in-memory calendar store from the current settings.
    ///
    /// Adds directories for calendars that have been added to settings, updates names for existing
    /// calendars, and removes directories for calendars that are no longer present in settings.
    pub async fn refresh_store(state: &mut State) -> anyhow::Result<()> {
        let State {
            store,
            settings,
            xdg,
            ..
        } = &mut *state;

        // detect added/updated calendars
        for (col_id, col) in settings.collections().iter() {
            for (cal_id, cal) in col.calendars() {
                match store.directory_mut(&Arc::new(cal_id.clone())) {
                    Some(dir) => dir.set_name(cal.name().clone()),
                    None => {
                        let dir = Self::load_calendar(xdg, col_id, col, cal_id, cal)?;
                        store.add(dir);
                    }
                }
            }
        }

        // detect removed calendars
        store.retain(|dir| settings.calendar(dir.id()).is_some());

        Ok(())
    }

    fn load_calendar(
        xdg: &BaseDirectories,
        col_id: &str,
        col: &CollectionSettings,
        cal_id: &str,
        cal: &CalendarSettings,
    ) -> anyhow::Result<CalDir> {
        let cal_id: Arc<String> = Arc::from(cal_id.to_owned());
        let col_path = col.path(xdg, col_id);
        let path = col_path.join(cal.folder());
        let mut dir = if path.exists() {
            CalDir::new_from_dir(cal_id.clone(), path.clone(), cal.name().clone()).with_context(
                || {
                    format!(
                        "Loading calendar {} from '{}' failed",
                        cal_id,
                        path.to_str().unwrap()
                    )
                },
            )?
        } else {
            tracing::warn!(
                "Creating empty calendar '{}' from non-existing directory {}",
                cal_id,
                path.to_str().unwrap()
            );
            CalDir::new_empty(cal_id.clone(), path, cal.name().clone())
        };

        // Workaround for a bug in Exchange/davmail:
        // Exchange sometimes sends events that include attendees but lack an organizer. davmail
        // does not repair that omission. When this happens and we are the organizer for the
        // collection, add our organizer information to such components so downstream code can rely
        // on it.
        let organizer = col.build_organizer();
        if let Some(organizer) = organizer {
            for comp in dir.files_mut().iter_mut().flat_map(|f| {
                f.component_with_mut(|c| {
                    c.rid().is_none() && c.organizer().is_none() && c.attendees().is_some()
                })
            }) {
                tracing::warn!(
                    "Making ourself the organizer of group-scheduled item {}",
                    comp.uid()
                );
                comp.set_organizer(Some(organizer.clone()));
            }
        }

        Ok(dir)
    }

    fn reload_from_file(
        state: &mut State,
        col_id: &String,
        cal_ids: Vec<String>,
    ) -> anyhow::Result<()> {
        // Delete all calendars of that collection from the in-memory store; they will be reloaded
        // from disk below.
        state
            .store_mut()
            .retain(|dir| !cal_ids.contains(&**dir.id()));

        // Load calendars again from file
        let col = state.settings().collections().get(col_id).unwrap();
        let mut dirs = vec![];
        for cal_id in cal_ids {
            let cal = col.all_calendars().get(&cal_id).unwrap();
            let dir = Self::load_calendar(state.xdg(), col_id, col, &cal_id, cal)?;
            dirs.push(dir);
        }

        // add them to store
        for dir in dirs {
            state.store_mut().add(dir);
        }
        Ok(())
    }

    /// Discovers a remote collection and return information about it.
    ///
    /// Returns a `SyncResult` describing the discovered collection with collection id `col_id` or
    /// an error. If `auth_url` is given, it is used to re-authenticate the user.
    pub async fn discover_collection(
        state: &mut State,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        sync::discover_collection(state, col_id, auth_url).await
    }

    /// Synchronizes a single collection with its remote source.
    ///
    /// On success returns a `SyncResult` describing the performed operations. If `auth_url` is
    /// given, it is used to re-authenticate the user.
    pub async fn sync_collection(
        state: &mut State,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        sync::sync_collection(state, col_id, auth_url).await
    }

    /// Reloads a collection from its remote source and refresh local files.
    ///
    /// Discards local files for the collection, re-fetches from the remote source, and returns
    /// the resulting `SyncResult`. If `auth_url` is given, it is used to re-authenticate.
    pub async fn reload_collection(
        state: &mut State,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        let res = sync::reload_collection(state, col_id, auth_url).await?;

        let col = state.settings().collections().get(col_id).unwrap();
        let cal_ids = col
            .all_calendars()
            .keys()
            .map(|id| id.to_string())
            .collect::<Vec<_>>();
        Self::reload_from_file(state, col_id, cal_ids)?;

        Ok(res)
    }

    /// Deletes a collection remotely and remove it from local settings.
    pub async fn delete_collection(state: &mut State, col_id: &String) -> anyhow::Result<()> {
        sync::delete_collection(state, col_id).await?;

        state.settings_mut().collections_mut().remove(col_id);
        Ok(())
    }

    /// Deletes a calendar from a collection both remotely and locally.
    pub async fn delete_calendar(
        state: &mut State,
        col_id: &String,
        cal_id: &String,
    ) -> anyhow::Result<()> {
        sync::delete_calendar(state, col_id, cal_id).await?;

        let col = state
            .settings_mut()
            .collections_mut()
            .get_mut(col_id)
            .ok_or_else(|| anyhow!("No collection '{}'", col_id))?;
        col.all_calendars_mut().remove(cal_id);
        Ok(())
    }

    /// Reloads a single calendar from its remote source and refreshes the local file.
    ///
    /// Discards local files for the calendar identified by `cal_id`, re-fetches from the remote
    /// source, and returns the resulting `SyncResult`. If `auth_url` is given, it is used to
    /// re-authenticate.
    pub async fn reload_calendar(
        state: &mut State,
        col_id: &String,
        cal_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        let res = sync::reload_calendar(state, col_id, cal_id, auth_url).await?;
        Self::reload_from_file(state, col_id, vec![cal_id.to_string()])?;
        Ok(res)
    }

    /// Synchronizes all collections.
    pub async fn sync_all(
        state: &mut State,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        let sync_res = sync::sync_all(state, auth_url).await?;

        state.last_reload = chrono::Utc::now().naive_utc();

        Ok(sync_res)
    }

    /// Returns a reference to the XDG base directories used for file storage.
    pub fn xdg(&self) -> &BaseDirectories {
        &self.xdg
    }

    /// Returns a clone of the `Locale` implementation.
    pub fn locale(&self) -> Arc<dyn Locale + Send + Sync> {
        self.locale.clone()
    }

    /// Returns an immutable reference to the in-memory calendar store.
    pub fn store(&self) -> &CalStore {
        &self.store
    }

    /// Returns a mutable reference to the in-memory calendar store.
    pub fn store_mut(&mut self) -> &mut CalStore {
        &mut self.store
    }

    /// Borrows the calendar store and personal alarms mutably at the same time.
    pub fn store_and_alarms_mut(&mut self) -> (&mut CalStore, &mut PersonalAlarms) {
        (&mut self.store, &mut self.personal_alarms)
    }

    /// Returns an immutable reference to personal alarms state.
    pub fn personal_alarms(&self) -> &PersonalAlarms {
        &self.personal_alarms
    }

    /// Returns a mutable reference to personal alarms state.
    pub fn personal_alarms_mut(&mut self) -> &mut PersonalAlarms {
        &mut self.personal_alarms
    }

    /// Returns an immutable reference to application settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Returns a mutable reference to application settings.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Returns an immutable reference to misc persisted state.
    pub fn misc(&self) -> &misc::Misc {
        &self.misc
    }

    /// Returns a mutable reference to misc persisted state.
    pub fn misc_mut(&mut self) -> &mut misc::Misc {
        &mut self.misc
    }

    /// Returns the timestamp of the last successful full synchronization.
    pub fn last_reload(&self) -> NaiveDateTime {
        self.last_reload
    }
}

/// Read and deserialize a TOML file from `filename`.
pub fn load_from_file<D: DeserializeOwned>(filename: &PathBuf) -> anyhow::Result<D> {
    debug!("Reading from {:?}", filename);
    let mut file = File::options()
        .read(true)
        .open(filename)
        .context(format!("open {filename:?}"))?;
    let mut data = String::new();
    file.read_to_string(&mut data)
        .context(format!("read {filename:?}"))?;
    toml::from_str(&data).context(format!("parse {filename:?}"))
}

/// Serialize `data` as TOML and write it to `filename`.
///
/// The function truncates existing content and creates the file if needed.
pub fn write_to_file<S: Serialize>(filename: &PathBuf, data: S) -> anyhow::Result<()> {
    debug!("Writing to {:?}", filename);
    let mut file = File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(filename)
        .context(format!("open {filename:?}"))?;
    file.write_all(
        toml::to_string(&data)
            .context(format!("serialize {filename:?}"))?
            .as_bytes(),
    )
    .context(format!("write {filename:?}"))?;
    Ok(())
}

/// Sets `XDG_DATA_HOME` to `data` and `XDG_CONFIG_HOME` to `config`, constructs a
/// `BaseDirectories` snapshot, then releases the lock before returning.
///
/// The entire set-and-snapshot region is performed while holding a lock, so concurrent test
/// threads cannot observe a partially-updated environment.
pub fn with_test_xdg(data: &std::path::Path, config: &std::path::Path) -> xdg::BaseDirectories {
    static XDG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    let _guard = XDG_LOCK.lock().unwrap();
    // SAFETY: the lock above ensures no other thread reads or writes these
    // variables while we hold it, so the unsynchronised write is safe within
    // the test-only context.
    unsafe {
        std::env::set_var("XDG_DATA_HOME", data);
        std::env::set_var("XDG_CONFIG_HOME", config);
    }
    xdg::BaseDirectories::with_prefix("")
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use eventix_ical::col::{CalDir, CalStore};

    use crate::{
        PersonalAlarms,
        misc::Misc,
        settings::{CollectionSettings, SyncerType},
    };

    use super::State;

    // --- helpers ---

    fn make_state(id: &str, name: &str) -> State {
        let mut store = CalStore::default();
        store.add(CalDir::new_empty(
            Arc::new(id.to_string()),
            PathBuf::from(format!("/tmp/{id}")),
            name.to_string(),
        ));
        State::new_for_test(store, Misc::new(PathBuf::default()))
    }

    // --- accessor tests ---

    #[test]
    fn state_store_accessors() {
        let mut state = make_state("cal1", "My Calendar");

        assert_eq!(state.store().directories().len(), 1);
        assert_eq!(state.store().directories()[0].name(), "My Calendar");

        // store_mut allows mutating the underlying store
        state.store_mut().add(CalDir::new_empty(
            Arc::new("cal2".to_string()),
            PathBuf::from("/tmp/cal2"),
            "Second".to_string(),
        ));
        assert_eq!(state.store().directories().len(), 2);
    }

    #[test]
    fn state_store_and_alarms_mut() {
        let mut state = make_state("cal1", "Cal");

        let (store, alarms) = state.store_and_alarms_mut();
        // Both references are accessible simultaneously.
        assert_eq!(store.directories().len(), 1);
        assert_eq!(*alarms, PersonalAlarms::default());
    }

    #[test]
    fn state_personal_alarms_accessors() {
        let mut state = make_state("cal1", "Cal");

        assert_eq!(*state.personal_alarms(), PersonalAlarms::default());
        // get_or_create via mutable reference creates an entry
        state.personal_alarms_mut().get_or_create("cal1");
        // The calendar entry now exists; get returns Some.
        assert!(state.personal_alarms().get("cal1").is_some());
    }

    #[test]
    fn state_settings_accessors() {
        let mut state = make_state("cal1", "Cal");

        // Default settings contain no collections.
        assert!(state.settings().collections().is_empty());

        // Insert a collection via settings_mut.
        state.settings_mut().collections_mut().insert(
            "col1".to_string(),
            CollectionSettings::new(SyncerType::FileSystem {
                path: "/tmp".to_string(),
            }),
        );
        assert!(state.settings().collections().contains_key("col1"));
    }

    #[test]
    fn state_misc_accessors() {
        let misc = Misc::new(PathBuf::default());
        let mut state = State::new_for_test(CalStore::default(), misc);

        // misc() returns a reference to the misc state.
        assert!(!state.misc().calendar_disabled(&"any".to_string()));

        // misc_mut() allows mutation.
        state.misc_mut().toggle_calendar(&"cal-x".to_string());
        assert!(state.misc().calendar_disabled(&"cal-x".to_string()));
    }

    #[test]
    fn state_locale_and_xdg_and_last_reload() {
        let before = chrono::Utc::now().naive_utc();
        let state = make_state("cal1", "Cal");
        let after = chrono::Utc::now().naive_utc();

        // locale() returns an Arc without panicking.
        let _locale = state.locale();
        // xdg() returns a valid BaseDirectories reference.
        let _xdg = state.xdg();
        // last_reload is set at construction time.
        let ts = state.last_reload();
        assert!(
            ts >= before && ts <= after,
            "last_reload must be set at construction time"
        );
    }
}
