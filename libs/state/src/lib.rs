// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod misc;
mod persalarms;
mod settings;
mod sync;

/// Utility helpers exposed to other crates and tests.
pub mod util;

use anyhow::{Context, anyhow};
use chrono::NaiveDateTime;
use chrono_tz::Tz;
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
    CalendarAlarmType, CalendarSettings, CollectionSettings, EmailAccount, Settings, SyncTimeBound,
    SyncTimeSpan, SyncerType,
};
pub use sync::{SyncColResult, SyncResult, Syncer, create_calendar_by_folder, log_file};

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

struct CollectionSyncPlan {
    col_id: String,
    index: usize,
    snapshot: CollectionSettings,
    token: Option<String>,
    protected_dirs: Vec<Arc<String>>,
}

impl State {
    async fn run_collection_sync_op<F, Fut>(
        state: &EventixState,
        col_id: &String,
        auth_url: Option<&String>,
        run: F,
    ) -> anyhow::Result<sync::SyncResult>
    where
        F: FnOnce(
            Arc<BaseDirectories>,
            usize,
            String,
            CollectionSettings,
            Option<String>,
            Option<String>,
        ) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<sync::SyncExecution>>,
    {
        let (plan, xdg) = {
            let mut state = state.lock().await;
            let plan = Self::prepare_collection_sync(&mut state, col_id)?;
            (plan, state.xdg.clone())
        };

        let (snapshot, mut syncer, res) = run(
            xdg,
            plan.index,
            plan.col_id.clone(),
            plan.snapshot.clone(),
            plan.token.clone(),
            auth_url.cloned(),
        )
        .await?;

        let mut state = state.lock().await;
        Self::release_collection_sync(&mut state, &plan);
        let mut sync_res = sync::SyncResult::default();
        sync::handle_sync_result(
            &mut state,
            &plan.col_id,
            &snapshot,
            &mut syncer,
            res,
            &mut sync_res,
        )
        .await;
        Ok(sync_res)
    }

    /// Creates a new in-memory application `State` by loading persisted data from the provided XDG
    /// base directories.
    ///
    /// Loads settings, personal alarms, misc state, and constructs the calendar store from all
    /// configured collections. Returns an error if any of the underlying loads fail.
    pub fn new(xdg: Arc<BaseDirectories>) -> anyhow::Result<Self> {
        Self::new_with_timezone(xdg, None)
    }

    /// Creates a new in-memory application `State`, optionally overriding the locale timezone.
    pub fn new_with_timezone(
        xdg: Arc<BaseDirectories>,
        tz_override: Option<Tz>,
    ) -> anyhow::Result<Self> {
        let settings = settings::Settings::load_from_file(&xdg).context("load settings")?;

        let personal_alarms = PersonalAlarms::new_from_dir(&xdg).context("load personal alarms")?;

        let misc = misc::Misc::load_from_file(&xdg).context("load misc state")?;
        let locale = match tz_override {
            Some(tz) => eventix_locale::new_with_timezone(&xdg, misc.locale_type(), tz)?,
            None => eventix_locale::new(&xdg, misc.locale_type())?,
        };

        let mut store = CalStore::default();
        let local_tz = locale.timezone();
        for (col_id, col) in settings.collections().iter() {
            for (cal_id, cal) in col.calendars() {
                let dir = Self::load_calendar(&xdg, col_id, col, cal_id, cal, local_tz)?;
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

    /// Reloads the `Locale` implementation from persisted misc state.
    ///
    /// Call this after the user changes language/locale preferences so that formatting and
    /// translations are updated without restarting the application. Returns an error if creating
    /// the locale fails.
    pub fn reload_locale(&mut self) -> anyhow::Result<()> {
        self.reload_locale_with_timezone(None)
    }

    /// Reloads the locale, optionally overriding the timezone used for formatting.
    pub fn reload_locale_with_timezone(&mut self, tz_override: Option<Tz>) -> anyhow::Result<()> {
        self.locale = match tz_override {
            Some(tz) => eventix_locale::new_with_timezone(&self.xdg, self.misc.locale_type(), tz)?,
            None => eventix_locale::new(&self.xdg, self.misc.locale_type())?,
        };
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
            locale,
            ..
        } = &mut *state;
        let local_tz = locale.timezone();

        // detect added/updated calendars
        for (col_id, col) in settings.collections().iter() {
            for (cal_id, cal) in col.calendars() {
                let cal_id = Arc::new(cal_id.clone());
                if store.directory(&cal_id).is_some() {
                    match store.try_directory_mut(&cal_id) {
                        Ok(dir) => dir.set_name(cal.name().clone()),
                        Err(eventix_ical::col::ColError::DirWriteProtected(_)) => {}
                        Err(err) => return Err(err.into()),
                    }
                } else {
                    let dir = Self::load_calendar(xdg, col_id, col, &cal_id, cal, local_tz)?;
                    store.add(dir);
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
        local_tz: &Tz,
    ) -> anyhow::Result<CalDir> {
        let cal_id: Arc<String> = Arc::from(cal_id.to_owned());
        let col_path = col.path(xdg, col_id);
        let path = col_path.join(cal.folder());
        let mut dir = if path.exists() {
            CalDir::new_from_dir(cal_id.clone(), path.clone(), cal.name().clone(), local_tz)
                .with_context(|| {
                    format!(
                        "Loading calendar {} from '{}' failed",
                        cal_id,
                        path.to_str().unwrap()
                    )
                })?
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

    fn prepare_collection_sync(
        state: &mut State,
        col_id: &String,
    ) -> anyhow::Result<CollectionSyncPlan> {
        let snapshot = state
            .settings()
            .collections()
            .get(col_id)
            .cloned()
            .ok_or_else(|| anyhow!("No collection '{}'", col_id))?;
        let index = state
            .settings()
            .collections()
            .keys()
            .enumerate()
            .find_map(|(idx, id)| (id == col_id).then_some(idx))
            .ok_or_else(|| anyhow!("No collection '{}'", col_id))?;
        let token = state.misc().collection_token(col_id).cloned();
        let protected_dirs = snapshot
            .all_calendars()
            .keys()
            .cloned()
            .map(Arc::new)
            .collect::<Vec<_>>();
        state
            .store_mut()
            .protect_directories(protected_dirs.clone())
            .map_err(anyhow::Error::from)?;
        Ok(CollectionSyncPlan {
            col_id: col_id.clone(),
            index,
            snapshot,
            token,
            protected_dirs,
        })
    }

    fn release_collection_sync(state: &mut State, plan: &CollectionSyncPlan) {
        state
            .store_mut()
            .unprotect_directories(plan.protected_dirs.clone());
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

    /// Deletes the remote calendar identified by `folder` and removes its local synced files.
    pub async fn delete_calendar_by_folder(
        state: &mut State,
        col_id: &String,
        folder: &String,
    ) -> anyhow::Result<()> {
        sync::delete_calendar_by_folder(state, col_id, folder).await
    }

    /// Synchronizes a single collection.
    pub async fn sync_collection(
        state: &EventixState,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        Self::run_collection_sync_op(state, col_id, auth_url, sync::run_sync_from_snapshot).await
    }

    /// Discovers a remote collection and returns information about it.
    pub async fn discover_collection(
        state: &EventixState,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        Self::run_collection_sync_op(state, col_id, auth_url, sync::run_discover_from_snapshot)
            .await
    }

    /// Reloads a collection from its remote source and refreshes local files.
    pub async fn reload_collection(
        state: &EventixState,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        Self::run_collection_sync_op(
            state,
            col_id,
            auth_url,
            sync::run_reload_collection_from_snapshot,
        )
        .await
    }

    /// Reloads a single calendar from its remote source and refreshes the local file.
    pub async fn reload_calendar(
        state: &EventixState,
        col_id: &String,
        cal_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        Self::run_collection_sync_op(state, col_id, auth_url, |xdg, idx, id, col, token, auth| {
            sync::run_reload_calendar_from_snapshot(xdg, idx, id, col, token, auth, cal_id)
        })
        .await
    }

    /// Synchronizes all collections.
    pub async fn sync_all(
        state: &EventixState,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        let col_ids = {
            let state = state.lock().await;
            state
                .settings()
                .collections()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };

        let mut sync_res = sync::SyncResult::default();
        for col_id in col_ids {
            let mut col_res = Self::sync_collection(state, &col_id, auth_url).await?;
            sync_res.changed |= col_res.changed;
            sync_res.collections.extend(col_res.collections.drain());
            sync_res.calendars.extend(col_res.calendars.drain());
        }

        let mut state = state.lock().await;
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

    /// Returns the user's local timezone as configured in the locale.
    pub fn timezone(&self) -> &Tz {
        self.locale.timezone()
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
