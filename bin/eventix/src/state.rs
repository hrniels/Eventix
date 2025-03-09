use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Context;
use chrono::NaiveDateTime;
use ical::col::{CalDir, CalStore};
use tokio::sync::Mutex;

use crate::settings::{self, Settings};

pub type EventixState = Arc<Mutex<State>>;

#[derive(Default)]
pub struct State {
    store: CalStore,
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
                    HashMap::new(),
                )
                .with_context(|| format!("Loading calendar {} from '{}' failed", id, cal.path()))?,
            );
        }

        let changed = self.store != store;

        self.store = store;
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
