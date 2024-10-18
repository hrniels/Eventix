use chrono::DateTime;
use chrono_tz::Tz;

use crate::objects::{CalComponent, CalEvent, CalItem, CalSource, CalTodo};

#[derive(Default)]
pub struct CalStore {
    sources: Vec<CalSource>,
}

impl CalStore {
    pub fn add(&mut self, source: CalSource) {
        self.sources.push(source);
    }

    pub fn sources(&self) -> &[CalSource] {
        &self.sources
    }

    pub fn items(&self) -> impl Iterator<Item = &CalItem> {
        self.sources.iter().map(|c| c.items().iter()).flatten()
    }

    pub fn items_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> impl Iterator<Item = (&CalComponent, DateTime<Tz>)> {
        self.sources
            .iter()
            .map(|c| c.items().iter())
            .flatten()
            .map(move |i| i.items_within(start, end))
            .flatten()
    }

    pub fn todos(&self) -> impl Iterator<Item = &CalTodo> {
        self.items().map(|i| i.todos()).flatten()
    }

    pub fn events(&self) -> impl Iterator<Item = &CalEvent> {
        self.items().map(|i| i.events()).flatten()
    }
}
