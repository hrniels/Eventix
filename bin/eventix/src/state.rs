use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Context;
use chrono::{Duration, NaiveDateTime};
use ical::{
    col::{CalDir, CalStore},
    objects::CalCompType,
};
use tokio::sync::Mutex;

use crate::settings;

pub type EventixState = Arc<Mutex<State>>;

#[derive(Default)]
pub struct State {
    store: CalStore,
    disabled_cals: Vec<String>,
    last_alarm_check: NaiveDateTime,
    last_reload: NaiveDateTime,
    last_calendar: HashMap<CalCompType, String>,
}

impl State {
    pub async fn reload(&mut self) -> anyhow::Result<bool> {
        let settings = settings::Settings::load_from_file()
            .await
            .context("load settings")?;

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
                CalDir::new_from_dir(
                    Arc::from(id.clone()),
                    PathBuf::from(cal.path.clone()),
                    cal.name.clone(),
                    props,
                )
                .with_context(|| format!("Loading calendar {} from '{}' failed", id, cal.path))?,
            );
        }

        let changed = self.store != store;
        let now = chrono::Utc::now().naive_utc();

        self.store = store;
        self.disabled_cals = disabled_cals;
        self.last_reload = now;
        self.last_calendar = settings.last_calendar;
        self.last_alarm_check = settings.last_alarm_check.unwrap_or(now - Duration::days(7));

        Ok(changed)
    }

    pub fn store(&self) -> &CalStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut CalStore {
        &mut self.store
    }

    pub fn disabled_cals(&self) -> &Vec<String> {
        &self.disabled_cals
    }

    pub fn toggle_calendar(&mut self, cal: &String) {
        if self.disabled_cals.contains(cal) {
            self.disabled_cals.retain(|d| d != cal);
        } else {
            self.disabled_cals.push(cal.to_string());
        }
    }

    pub fn last_reload(&self) -> NaiveDateTime {
        self.last_reload
    }

    pub fn last_alarm_check(&self) -> NaiveDateTime {
        self.last_alarm_check
    }

    pub fn set_last_alarm_check(&mut self, datetime: NaiveDateTime) {
        self.last_alarm_check = datetime;
    }

    pub fn last_calendar(&self) -> &HashMap<CalCompType, String> {
        &self.last_calendar
    }

    pub fn set_last_calendar(&mut self, ty: CalCompType, cal: String) {
        if let Some(e) = self.last_calendar.get_mut(&ty) {
            *e = cal;
        } else {
            self.last_calendar.insert(ty, cal);
        }
    }
}
