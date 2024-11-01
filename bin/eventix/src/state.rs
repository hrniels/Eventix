use std::sync::{Arc, Mutex};

use ical::col::CalStore;

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
