use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use chrono::NaiveDateTime;
use ical::col::{CalDir, CalStore};
use tokio::sync::Mutex;

use crate::{
    persalarms::PersonalAlarms,
    settings::{self, Settings},
    sync,
};

pub type EventixState = Arc<Mutex<State>>;

pub struct State {
    store: CalStore,
    personal_alarms: PersonalAlarms,
    settings: settings::Settings,
    last_reload: NaiveDateTime,
}

impl State {
    pub async fn new() -> anyhow::Result<Self> {
        let settings = settings::Settings::load_from_file()
            .await
            .context("load settings")?;

        let personal_alarms = PersonalAlarms::new_from_dir().context("load personal alarms")?;

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
            last_reload: chrono::Utc::now().naive_utc(),
        })
    }

    pub async fn reload(state: EventixState) -> anyhow::Result<bool> {
        // first reload the settings and personal alarms
        let mut changed = {
            let mut state = state.lock().await;
            let settings = settings::Settings::load_from_file()
                .await
                .context("load settings")?;

            let personal_alarms = PersonalAlarms::new_from_dir().context("load personal alarms")?;

            let changed = state.personal_alarms != personal_alarms;

            state.personal_alarms = personal_alarms;
            state.settings = settings;

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

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn last_reload(&self) -> NaiveDateTime {
        self.last_reload
    }
}
