mod misc;
mod persalarms;
mod settings;
mod sync;
pub mod util;

use anyhow::{Context, anyhow};
use chrono::NaiveDateTime;
use eventix_ical::{
    col::{CalDir, CalStore},
    objects::{EventLike, UpdatableEventLike},
};
use once_cell::sync::Lazy;
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
pub use sync::{SyncCalResult, SyncResult, Syncer};

/// Global lock for all sync/discover/delete/... operations on collections and calendars.
///
/// Although we have another lock for the state, we need a global lock in addition to include
/// changes to file system state, used network ports, and the like. For example, we cannot allow
/// multiple sync operations to happen in parallel or a discover and delete operation in parallel
/// and so on.
///
/// Therefore, the global lock is taken first for all of these operations ensuring that only one of
/// them can be done at a time and within these operations we take the state lock when required to
/// ensure mutual exclusion with other state accessess (e.g., when reading the state or adding a
/// new event).
static GLOBAL_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub type EventixState = Arc<Mutex<State>>;

pub struct State {
    xdg: Arc<BaseDirectories>,
    store: CalStore,
    personal_alarms: PersonalAlarms,
    settings: settings::Settings,
    misc: misc::Misc,
    last_reload: NaiveDateTime,
}

impl State {
    pub fn new(xdg: Arc<BaseDirectories>) -> anyhow::Result<Self> {
        let settings = settings::Settings::load_from_file(&xdg).context("load settings")?;

        let personal_alarms = PersonalAlarms::new_from_dir(&xdg).context("load personal alarms")?;

        let misc = misc::Misc::load_from_file(&xdg).context("load misc state")?;

        let mut store = CalStore::default();
        for (col_id, col) in settings.collections().iter() {
            for (cal_id, cal) in col.calendars() {
                let dir = Self::load_calendar(&xdg, col_id, col, cal_id, cal)?;
                store.add(dir);
            }
        }

        Ok(Self {
            xdg,
            settings,
            personal_alarms,
            store,
            misc,
            last_reload: chrono::Utc::now().naive_utc(),
        })
    }

    pub async fn refresh_store(state: EventixState) -> anyhow::Result<()> {
        let _lock = GLOBAL_LOCK.lock().await;
        let mut state = state.lock().await;

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

        // workaround for a bug in Exchange/davmail: apparently, Exchange sends events with
        // attendees, but without organizer to davmail and davmail does not repair it. As this
        // seems to *only* happen if we are the organizer, we implicitly add ourself as an
        // organizer to these events.
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

    pub async fn discover_collection(
        state: EventixState,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        let _lock = GLOBAL_LOCK.lock().await;
        sync::discover_collection(state, col_id, auth_url).await
    }

    pub async fn sync_collection(
        state: EventixState,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        let _lock = GLOBAL_LOCK.lock().await;
        sync::sync_collection(state, col_id, auth_url).await
    }

    pub async fn reload_collection(
        state: EventixState,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        let _lock = GLOBAL_LOCK.lock().await;
        sync::reload_collection(state, col_id, auth_url).await
    }

    pub async fn delete_calendar(
        state: EventixState,
        col_id: &String,
        cal_id: &String,
    ) -> anyhow::Result<()> {
        let _lock = GLOBAL_LOCK.lock().await;

        sync::delete_calendar(state.clone(), col_id, cal_id).await?;

        let mut state = state.lock().await;
        let col = state
            .settings_mut()
            .collections_mut()
            .get_mut(col_id)
            .ok_or_else(|| anyhow!("No collection '{}'", col_id))?;
        col.all_calendars_mut().remove(cal_id);
        Ok(())
    }

    pub async fn reload_calendar(
        state: EventixState,
        col_id: &String,
        cal_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        let _lock = GLOBAL_LOCK.lock().await;
        sync::reload_calendar(state, col_id, cal_id, auth_url).await
    }

    pub async fn reload(
        state: EventixState,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        let _lock = GLOBAL_LOCK.lock().await;

        // first reload the settings and personal alarms
        let changed = {
            let mut state = state.lock().await;

            let settings =
                settings::Settings::load_from_file(&state.xdg).context("load settings")?;
            let personal_alarms =
                PersonalAlarms::new_from_dir(&state.xdg).context("load personal alarms")?;
            let misc = misc::Misc::load_from_file(&state.xdg).context("load misc state")?;

            let changed = state.personal_alarms != personal_alarms || state.misc != misc;

            state.personal_alarms = personal_alarms;
            state.settings = settings;
            state.misc = misc;

            changed
        };

        // now synchronize and update the store
        let mut sync_res = sync::sync_all(state.clone(), auth_url).await?;
        sync_res.changed |= changed;

        // remember last reload
        state.lock().await.last_reload = chrono::Utc::now().naive_utc();

        Ok(sync_res)
    }

    pub fn xdg(&self) -> &BaseDirectories {
        &self.xdg
    }

    pub fn store(&self) -> &CalStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut CalStore {
        &mut self.store
    }

    pub fn store_and_alarms_mut(&mut self) -> (&mut CalStore, &mut PersonalAlarms) {
        (&mut self.store, &mut self.personal_alarms)
    }

    pub fn personal_alarms(&self) -> &PersonalAlarms {
        &self.personal_alarms
    }

    pub fn personal_alarms_mut(&mut self) -> &mut PersonalAlarms {
        &mut self.personal_alarms
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn misc(&self) -> &misc::Misc {
        &self.misc
    }

    pub fn misc_mut(&mut self) -> &mut misc::Misc {
        &mut self.misc
    }

    pub fn last_reload(&self) -> NaiveDateTime {
        self.last_reload
    }
}

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
