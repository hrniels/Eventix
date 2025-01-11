use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Context;
use chrono::{Duration, NaiveDateTime};
use ical::{
    col::{CalSource, CalStore},
    objects::CalCompType,
};
use tokio::sync::{Mutex, MutexGuard};

use crate::settings;

#[derive(Clone, Default)]
pub struct State {
    store: Arc<Mutex<CalStore>>,
    disabled_cals: Arc<Mutex<Vec<String>>>,
    last_alarm_check: Arc<Mutex<NaiveDateTime>>,
    last_calendar: Arc<Mutex<HashMap<CalCompType, String>>>,
}

impl State {
    pub async fn reload(&self) -> anyhow::Result<()> {
        let settings = settings::Settings::load_from_file().context("load settings")?;

        let mut disabled_cals = Vec::new();
        let mut store = CalStore::default();
        for (id, cal) in &settings.calendars {
            if cal.disabled.unwrap_or(false) {
                disabled_cals.push(id.clone());
            }

            let mut props = HashMap::new();
            props.insert("fgcolor".to_string(), cal.fgcolor.clone());
            props.insert("bgcolor".to_string(), cal.bgcolor.clone());
            if let Some(types) = &cal.types {
                props.insert("types".to_string(), serde_json::to_string(types).unwrap());
            }

            store.add(
                CalSource::new_from_dir(
                    Arc::from(id.clone()),
                    PathBuf::from(cal.path.clone()),
                    cal.name.clone(),
                    props,
                )
                .with_context(|| format!("Loading calendar {} from '{}' failed", id, cal.path))?,
            );
        }

        *self.store().lock().await = store;
        *self.disabled_cals().lock().await = disabled_cals;
        *self.last_calendar.lock().await = settings.last_calendar;
        *self.last_alarm_check.lock().await = settings
            .last_alarm_check
            .unwrap_or(chrono::Utc::now().naive_utc() - Duration::days(7));

        Ok(())
    }

    pub fn store(&self) -> &Arc<Mutex<CalStore>> {
        &self.store
    }

    pub fn disabled_cals(&self) -> &Arc<Mutex<Vec<String>>> {
        &self.disabled_cals
    }

    pub fn last_alarm_check(&self) -> &Arc<Mutex<NaiveDateTime>> {
        &self.last_alarm_check
    }

    pub fn last_calendar(&self) -> &Arc<Mutex<HashMap<CalCompType, String>>> {
        &self.last_calendar
    }

    pub async fn acquire_store_and_disabled(
        &self,
    ) -> (MutexGuard<'_, CalStore>, MutexGuard<'_, Vec<String>>) {
        let disabled = self.disabled_cals.lock().await;
        let store = self.store.lock().await;
        (store, disabled)
    }
}
