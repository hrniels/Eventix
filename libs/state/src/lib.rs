// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod crypto;
mod migration;
mod misc;
mod persalarms;
mod settings;
mod sync;

/// Toolkit-independent calendar mutations.
pub mod actions;

/// Microsoft device authentication and background monitoring.
pub mod auth;

/// Utility helpers exposed to other crates and tests.
pub mod util;

/// The current version of the persisted state files.
pub const CURRENT_VERSION: u32 = 4;

use anyhow::Context;
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
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Mutex;
use tracing::debug;
use xdg::BaseDirectories;

use actions::{
    Action, LockedAction,
    calendar::{
        CreateCalendar, CreateCollection, DeleteCalendar, DeleteCalendarByFolder, DeleteCollection,
        DiscoverCollection, ReloadCalendar, ReloadCollection, SaveCollection,
        StoreO365RefreshToken, SyncAll, SyncCollection,
    },
    items::PushItems,
};

pub use actions::items::{PushProgress, PushResult};

pub use crypto::{
    decrypt_password, encrypt_password, generate_local_password, retrieve_portal_secret,
};
pub use misc::Misc;
pub use persalarms::{PersonalAlarms, PersonalCalendarAlarms};
pub use settings::{
    CalendarAlarmType, CalendarSettings, CollectionSettings, EmailAccount, EncryptedPassword,
    Preferences, PreferencesError, Settings, SyncTimeBound, SyncTimeSpan, SyncerType,
};
pub use sync::{
    SyncCancellation, SyncColResult, SyncProgress, SyncResult, Syncer, create_calendar_by_folder,
    log_file,
};

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
            Some(tz) => {
                eventix_locale::new_with_timezone(&xdg, settings.preferences().locale_type, tz)?
            }
            None => eventix_locale::new(&xdg, settings.preferences().locale_type)?,
        };

        let mut store = CalStore::default();
        for (col_id, col) in settings.collections().iter() {
            let col_read_only = col.is_read_only();
            for (cal_id, cal) in col.calendars() {
                let dir = Self::load_calendar(&xdg, col_id, col, cal_id, cal, col_read_only)?;
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

    /// Reloads the `Locale` implementation from the current settings.
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
            Some(tz) => eventix_locale::new_with_timezone(
                &self.xdg,
                self.settings.preferences().locale_type,
                tz,
            )?,
            None => eventix_locale::new(&self.xdg, self.settings.preferences().locale_type)?,
        };
        Ok(())
    }

    /// Persists user preferences and updates translations without changing the timezone.
    ///
    /// Returns an error for invalid values, unavailable translations, or persistence failures.
    /// On failure, the previous preferences and loaded locale remain active.
    pub fn save_preferences(&mut self, preferences: Preferences) -> anyhow::Result<()> {
        let previous = self.settings.preferences();
        let locale = self.locale.clone();
        self.settings.set_preferences(preferences)?;
        let result = self
            .reload_locale_with_timezone(Some(*locale.timezone()))
            .and_then(|()| self.settings.write_to_file());
        if result.is_err() {
            self.settings.set_preferences(previous)?;
            self.locale = locale;
        }
        result
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
            let col_read_only = col.is_read_only();
            for (cal_id, cal) in col.calendars() {
                let cal_id = Arc::new(cal_id.clone());
                if store.directory(&cal_id).is_some() {
                    match store.directory_mut(&cal_id) {
                        Ok(dir) => dir.set_name(cal.name().clone()),
                        Err(eventix_ical::col::ColError::DirWriteProtected(_)) => {}
                        Err(err) => return Err(err.into()),
                    }
                } else {
                    let dir = Self::load_calendar(xdg, col_id, col, &cal_id, cal, col_read_only)?;
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
        read_only: bool,
    ) -> anyhow::Result<CalDir> {
        let cal_id: Arc<String> = Arc::from(cal_id.to_owned());
        let col_path = col.path(xdg, col_id);
        let path = col_path.join(cal.folder());
        let mut dir = if path.exists() {
            CalDir::new_from_dir(cal_id.clone(), path.clone(), cal.name().clone(), read_only)
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
            CalDir::new_empty(cal_id.clone(), path, cal.name().clone(), read_only)
        };

        // Workaround for a bug in Exchange/davmail:
        // Exchange sometimes sends events that include attendees but lack an organizer. davmail
        // does not repair that omission. When this happens and we are the organizer for the
        // collection, add our organizer information to such components so downstream code can rely
        // on it.
        let organizer = col.build_organizer();
        if let Some(organizer) = organizer
            // we skip read-only collections here, but that's okay, because if it's read-only being
            // the organizer would not help us to do any changes anyway.
            && let Ok(files) = dir.files_mut()
        {
            for comp in files.iter_mut().flat_map(|f| {
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

    /// Creates a collection, persists its settings and refreshes the local store.
    ///
    /// CalDAV collections with a username require a nonempty password, which is encrypted.
    /// O365 collections receive a generated encrypted password. Returns an error for invalid or
    /// duplicate IDs, credential failures, persistence failures or store refresh failures.
    /// Failed persistence removes the new collection from in-memory settings.
    pub async fn create_collection(
        state: &EventixState,
        id: &str,
        syncer: SyncerType,
        password: Option<&str>,
    ) -> anyhow::Result<()> {
        Box::new(CreateCollection {
            id: id.to_owned(),
            syncer,
            password: password.map(str::to_owned),
        })
        .execute(state)
        .await
    }

    /// Saves a collection's synchronizer settings and refreshes the local store.
    ///
    /// A missing password preserves current credentials; a supplied CalDAV password is encrypted.
    /// Returns an error if the collection is missing, its backend changes, encryption fails,
    /// persistence fails, or the store cannot be refreshed. Failed persistence restores the previous
    /// in-memory settings.
    pub async fn save_collection(
        state: &EventixState,
        col_id: &String,
        syncer: SyncerType,
        password: Option<&str>,
    ) -> anyhow::Result<()> {
        Box::new(SaveCollection {
            col_id: col_id.to_owned(),
            syncer,
            password: password.map(str::to_owned),
        })
        .execute(state)
        .await
    }

    /// Encrypts and persists a Microsoft refresh token for an O365 collection.
    pub async fn store_o365_refresh_token(
        state: &EventixState,
        col_id: &String,
        token: &str,
    ) -> anyhow::Result<()> {
        Box::new(StoreO365RefreshToken {
            col_id: col_id.to_owned(),
            token: token.to_owned(),
        })
        .execute(state)
        .await
    }

    /// Creates an enabled calendar in a writable collection and refreshes the local store.
    ///
    /// Trims the display name and selects a sanitized folder unused by configured calendars or
    /// existing files. Returns an error for empty names, missing or unsupported collections,
    /// read-only backends, filesystem or backend failures, persistence failures or refresh failures.
    /// Failures before persistence remove the new in-memory settings but may leave backend data.
    pub async fn create_calendar(
        state: &EventixState,
        id: &String,
        name: &str,
    ) -> anyhow::Result<()> {
        Box::new(CreateCalendar {
            id: id.to_owned(),
            name: name.to_owned(),
        })
        .execute(state)
        .await
    }

    /// Deletes a collection remotely and remove it from local settings.
    pub async fn delete_collection(state: &mut State, col_id: &String) -> anyhow::Result<()> {
        Box::new(DeleteCollection {
            col_id: col_id.to_owned(),
        })
        .execute_locked(state)
        .await
    }

    /// Deletes a calendar from a collection both remotely and locally.
    pub async fn delete_calendar(
        state: &mut State,
        col_id: &String,
        cal_id: &String,
    ) -> anyhow::Result<()> {
        Box::new(DeleteCalendar {
            col_id: col_id.to_owned(),
            cal_id: cal_id.to_owned(),
        })
        .execute_locked(state)
        .await
    }

    /// Deletes the remote calendar identified by `folder` and removes its local synced files.
    pub async fn delete_calendar_by_folder(
        state: &mut State,
        col_id: &String,
        folder: &String,
    ) -> anyhow::Result<()> {
        Box::new(DeleteCalendarByFolder {
            col_id: col_id.to_owned(),
            folder: folder.to_owned(),
        })
        .execute_locked(state)
        .await
    }

    /// Synchronizes one collection, retaining local changes if cancellation interrupts it.
    ///
    /// Returns only after backend processes have stopped and the local store has been reconciled.
    /// Cancellation preserves collection errors and is reported in the returned result.
    pub async fn sync_collection(
        state: &EventixState,
        col_id: &String,
        auth_url: Option<&String>,
        cancellation: &SyncCancellation,
    ) -> anyhow::Result<SyncResult> {
        Box::new(SyncCollection {
            col_id: col_id.to_owned(),
            auth_url: auth_url.cloned(),
            cancellation: cancellation.clone(),
        })
        .execute(state)
        .await
    }

    /// Discovers a remote collection and returns information about it.
    ///
    /// Cancellation stops backend work and reconciles local files before returning a cancelled
    /// result, preserving existing collection errors.
    pub async fn discover_collection(
        state: &EventixState,
        col_id: &String,
        auth_url: Option<&String>,
        cancellation: &SyncCancellation,
    ) -> anyhow::Result<SyncResult> {
        Box::new(DiscoverCollection {
            col_id: col_id.to_owned(),
            auth_url: auth_url.cloned(),
            cancellation: cancellation.clone(),
        })
        .execute(state)
        .await
    }

    /// Reloads a collection from its remote source and refreshes local files.
    ///
    /// Cancellation stops remaining reload stages and reconciles local files before returning a
    /// cancelled result. Files already deleted or downloaded are not restored to their old state.
    pub async fn reload_collection(
        state: &EventixState,
        col_id: &String,
        auth_url: Option<&String>,
        cancellation: &SyncCancellation,
    ) -> anyhow::Result<SyncResult> {
        Box::new(ReloadCollection {
            col_id: col_id.to_owned(),
            auth_url: auth_url.cloned(),
            cancellation: cancellation.clone(),
        })
        .execute(state)
        .await
    }

    /// Reloads a single calendar from its remote source and refreshes the local file.
    ///
    /// Cancellation stops remaining reload stages and reconciles local files before returning a
    /// cancelled result. Files already deleted or downloaded are not restored to their old state.
    pub async fn reload_calendar(
        state: &EventixState,
        col_id: &String,
        cal_id: &String,
        auth_url: Option<&String>,
        cancellation: &SyncCancellation,
    ) -> anyhow::Result<SyncResult> {
        Box::new(ReloadCalendar {
            col_id: col_id.to_owned(),
            cal_id: cal_id.to_owned(),
            auth_url: auth_url.cloned(),
            cancellation: cancellation.clone(),
        })
        .execute(state)
        .await
    }

    /// Pushes selected resources, retaining individual outcomes on failure or cancellation.
    ///
    /// Recurrence identities are ignored. Network work runs without holding the state lock.
    /// Unattempted items are absent from the result; cancellation never rolls back remote writes.
    /// Only selected files are uploaded; deletions and unrelated remote changes require normal
    /// synchronization. Invalid identities and calendars without remote write support are rejected
    /// before uploading. Interrupted writes can have uncertain outcomes and should be reconciled
    /// with normal synchronization before retrying.
    pub async fn push_items(
        state: &EventixState,
        items: &[actions::items::ItemId],
        progress: Option<&dyn PushProgress>,
        cancellation: &SyncCancellation,
    ) -> anyhow::Result<PushResult> {
        Box::new(PushItems {
            items: items.to_vec(),
            progress,
            cancellation: cancellation.clone(),
        })
        .execute(state)
        .await
    }

    /// Synchronizes all collections until completion or cancellation.
    ///
    /// Keeps completed results and skips remaining collections after cancellation. Interrupted
    /// collections are reconciled before returning, without counting them as completed progress.
    /// Only a fully completed operation advances the last-reload timestamp.
    pub async fn sync_all_cancellable(
        state: &EventixState,
        auth_url: Option<&String>,
        progress: Option<&dyn SyncProgress>,
        cancellation: &SyncCancellation,
    ) -> anyhow::Result<SyncResult> {
        Box::new(SyncAll {
            auth_url: auth_url.cloned(),
            progress,
            cancellation: cancellation.clone(),
        })
        .execute(state)
        .await
    }

    /// Synchronizes all collections.
    ///
    /// The optional observer receives the initial collection count and an update after each
    /// collection's result has been applied. Collection-level errors count as completed work
    /// and are included in the returned result. An operation-level error stops synchronization
    /// immediately without reporting the failed collection as ready.
    pub async fn sync_all(
        state: &EventixState,
        auth_url: Option<&String>,
        progress: Option<&dyn SyncProgress>,
    ) -> anyhow::Result<SyncResult> {
        Self::sync_all_cancellable(state, auth_url, progress, &SyncCancellation::default()).await
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
    migration::migrate_if_needed(filename).context("migrating file")?;
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
        .truncate(false)
        .mode(0o600)
        .open(filename)
        .context(format!("open {filename:?}"))?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .context(format!("set permissions on {filename:?}"))?;
    file.set_len(0).context(format!("truncate {filename:?}"))?;
    file.write_all(
        toml::to_string(&data)
            .context(format!("serialize {filename:?}"))?
            .as_bytes(),
    )
    .context(format!("write {filename:?}"))?;
    Ok(())
}

/// Sets `XDG_DATA_HOME` to `data` and `XDG_CONFIG_HOME` to `config`, constructs a
/// `BaseDirectories` snapshot for `prefix`, then releases the lock before returning.
///
/// The entire set-and-snapshot region is performed while holding a lock, so concurrent test
/// threads cannot observe a partially-updated environment.
pub fn with_test_xdg_prefix(
    data: &std::path::Path,
    config: &std::path::Path,
    prefix: &str,
) -> xdg::BaseDirectories {
    static XDG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    let _guard = XDG_LOCK.lock().unwrap();
    // SAFETY: the lock above ensures no other thread reads or writes these
    // variables while we hold it, so the unsynchronised write is safe within
    // the test-only context.
    unsafe {
        std::env::set_var("XDG_DATA_HOME", data);
        std::env::set_var("XDG_CONFIG_HOME", config);
    }
    xdg::BaseDirectories::with_prefix(prefix)
}

/// Sets `XDG_DATA_HOME` to `data` and `XDG_CONFIG_HOME` to `config`, constructs a
/// `BaseDirectories` snapshot, then releases the lock before returning.
///
/// The entire set-and-snapshot region is performed while holding a lock, so concurrent test
/// threads cannot observe a partially-updated environment.
pub fn with_test_xdg(data: &std::path::Path, config: &std::path::Path) -> xdg::BaseDirectories {
    with_test_xdg_prefix(data, config, "")
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use eventix_ical::col::{CalDir, CalStore};

    use crate::{
        EmailAccount, PersonalAlarms, SyncTimeSpan, encrypt_password,
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
            false,
        ));
        State::new_for_test(store, Misc::new(PathBuf::default()))
    }

    fn make_o365_state(path: PathBuf) -> State {
        let mut state = State::new_for_test(CalStore::default(), Misc::new(path));
        let password = encrypt_password(b"eventix test secret", "P@ssw0rd").unwrap();
        state.settings_mut().collections_mut().insert(
            "o365".to_string(),
            CollectionSettings::new(SyncerType::O365 {
                email: EmailAccount::new("User".to_string(), "user@example.com".to_string()),
                read_only: false,
                password,
                time_span: SyncTimeSpan::default(),
            }),
        );
        state
    }

    // --- accessor tests ---

    #[tokio::test]
    async fn stores_o365_refresh_token_encrypted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("misc.toml");
        let state = Arc::new(tokio::sync::Mutex::new(make_o365_state(path.clone())));

        State::store_o365_refresh_token(&state, &"o365".to_string(), "plain-refresh-token")
            .await
            .unwrap();

        let state = state.lock().await;
        let token = state.misc().collection_token(&"o365".to_string()).unwrap();
        assert!(token.starts_with("{AES}"));
        assert!(
            !std::fs::read_to_string(path)
                .unwrap()
                .contains("plain-refresh-token")
        );
    }

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
            false,
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
