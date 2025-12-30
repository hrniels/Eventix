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

pub type EventixState = Arc<Mutex<State>>;

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

    pub fn reload_locale(&mut self) -> anyhow::Result<()> {
        self.locale = eventix_locale::new(&self.xdg, self.misc.locale_type())?;
        Ok(())
    }

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

    fn reload_from_file(
        state: &mut State,
        col_id: &String,
        cal_ids: Vec<String>,
    ) -> anyhow::Result<()> {
        // delete all calendars of that collection
        state
            .store_mut()
            .retain(|dir| !cal_ids.contains(&**dir.id()));

        // load calendars again from file
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

    pub async fn discover_collection(
        state: &mut State,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        sync::discover_collection(state, col_id, auth_url).await
    }

    pub async fn sync_collection(
        state: &mut State,
        col_id: &String,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        sync::sync_collection(state, col_id, auth_url).await
    }

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

    pub async fn delete_collection(state: &mut State, col_id: &String) -> anyhow::Result<()> {
        sync::delete_collection(state, col_id).await?;

        state.settings_mut().collections_mut().remove(col_id);
        Ok(())
    }

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

    pub async fn sync_all(
        state: &mut State,
        auth_url: Option<&String>,
    ) -> anyhow::Result<sync::SyncResult> {
        let sync_res = sync::sync_all(state, auth_url).await?;

        // remember last reload
        state.last_reload = chrono::Utc::now().naive_utc();

        Ok(sync_res)
    }

    pub fn xdg(&self) -> &BaseDirectories {
        &self.xdg
    }

    pub fn locale(&self) -> Arc<dyn Locale + Send + Sync> {
        self.locale.clone()
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
