use std::sync::Arc;

use ical::col::CalStore;

#[derive(Clone)]
pub struct State {
    store: Arc<CalStore>,
}

impl State {
    pub fn new(store: Arc<CalStore>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &Arc<CalStore> {
        &self.store
    }
}
