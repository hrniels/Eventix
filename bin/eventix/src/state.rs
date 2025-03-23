use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use chrono::NaiveDateTime;
use ical::col::{CalDir, CalStore};
use tokio::sync::Mutex;

use crate::{
    persalarms::PersonalAlarms,
    settings::{self, Settings},
};

pub type EventixState = Arc<Mutex<State>>;

#[derive(Default)]
pub struct State {
    store: CalStore,
    personal_alarms: PersonalAlarms,
    settings: settings::Settings,
    last_reload: NaiveDateTime,
}

impl State {
    pub async fn reload(&mut self) -> anyhow::Result<bool> {
        let settings = settings::Settings::load_from_file()
            .await
            .context("load settings")?;

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

        let personal_alarms = PersonalAlarms::new_from_dir().context("load personal alarms")?;

        let changed = self.store != store || self.personal_alarms != personal_alarms;

        self.store = store;
        self.personal_alarms = personal_alarms;
        self.settings = settings;
        self.last_reload = chrono::Utc::now().naive_utc();

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
