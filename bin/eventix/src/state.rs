use std::sync::Arc;

use ical::col::CalStore;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct State {
    store: Arc<Mutex<CalStore>>,
}

impl State {
    pub fn new(store: Arc<Mutex<CalStore>>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &Arc<Mutex<CalStore>> {
        &self.store
    }
}
