use std::{collections::HashMap, sync::Arc};

use chrono::NaiveDateTime;
use ical::{col::CalStore, objects::CalCompType};
use tokio::sync::{Mutex, MutexGuard};

#[derive(Clone)]
pub struct State {
    store: Arc<Mutex<CalStore>>,
    disabled_cals: Arc<Mutex<Vec<String>>>,
    last_alarm_check: Arc<Mutex<NaiveDateTime>>,
    last_calendar: Arc<Mutex<HashMap<CalCompType, String>>>,
}

impl State {
    pub fn new(
        store: Arc<Mutex<CalStore>>,
        disabled_cals: Arc<Mutex<Vec<String>>>,
        last_alarm_check: Arc<Mutex<NaiveDateTime>>,
        last_calendar: Arc<Mutex<HashMap<CalCompType, String>>>,
    ) -> Self {
        Self {
            store,
            disabled_cals,
            last_alarm_check,
            last_calendar,
        }
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
