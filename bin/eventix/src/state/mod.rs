mod misc;
mod persalarms;
mod settings;

use anyhow::Context;
use chrono::NaiveDateTime;
use ical::col::{CalDir, CalStore};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Mutex;

use crate::sync;

pub use persalarms::{PersonalAlarms, PersonalCalendarAlarms};
pub use settings::{CalendarSettings, Settings, Syncer};

pub type EventixState = Arc<Mutex<State>>;

pub struct State {
    store: CalStore,
    personal_alarms: PersonalAlarms,
    settings: settings::Settings,
    misc: misc::Misc,
    last_reload: NaiveDateTime,
}

impl State {
    pub fn new() -> anyhow::Result<Self> {
        let settings = settings::Settings::load_from_file().context("load settings")?;

        let personal_alarms = PersonalAlarms::new_from_dir().context("load personal alarms")?;

        let misc = misc::Misc::load_from_file().context("load misc state")?;

        let mut store = CalStore::default();
        for (id, cal) in settings.calendars() {
            store.add(
                CalDir::new_from_dir(
                    Arc::from(id.clone()),
                    PathBuf::from(cal.path().clone()),
                    cal.name().clone(),
                )
                .with_context(|| format!("Loading calendar {} from '{}' failed", id, cal.path()))?,
            );
        }

        Ok(Self {
            settings,
            personal_alarms,
            store,
            misc,
            last_reload: chrono::Utc::now().naive_utc(),
        })
    }

    pub async fn reload(state: EventixState) -> anyhow::Result<bool> {
        // first reload the settings and personal alarms
        let mut changed = {
            let mut state = state.lock().await;

            let settings = settings::Settings::load_from_file().context("load settings")?;
            let personal_alarms = PersonalAlarms::new_from_dir().context("load personal alarms")?;
            let misc = misc::Misc::load_from_file().context("load misc state")?;

            let changed = state.personal_alarms != personal_alarms || state.misc != misc;

            state.personal_alarms = personal_alarms;
            state.settings = settings;
            state.misc = misc;

            changed
        };

        // now synchronize and update the store
        changed |= sync::sync_all(state.clone()).await;

        // remember last reload
        state.lock().await.last_reload = chrono::Utc::now().naive_utc();

        Ok(changed)
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
    let mut file = File::options()
        .read(true)
        .open(filename)
        .context(format!("open {:?}", filename))?;
    let mut data = String::new();
    file.read_to_string(&mut data)
        .context(format!("read {:?}", filename))?;
    toml::from_str(&data).context(format!("parse {:?}", filename))
}

pub fn write_to_file<S: Serialize>(filename: &PathBuf, data: S) -> anyhow::Result<()> {
    let mut file = File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(filename)
        .context(format!("open {:?}", filename))?;
    file.write_all(
        toml::to_string(&data)
            .context(format!("serialize {:?}", filename))?
            .as_bytes(),
    )
    .context(format!("write {:?}", filename))?;
    Ok(())
}
