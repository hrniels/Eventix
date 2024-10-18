use chrono::DateTime;
use chrono_tz::Tz;

use crate::col::{CalItem, CalSource};
use crate::objects::{CalComponent, CalEvent, CalTodo};

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
        self.sources.iter().flat_map(|c| c.items().iter())
    }

    pub fn items_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> impl Iterator<Item = (&CalComponent, DateTime<Tz>)> {
        self.sources
            .iter()
            .flat_map(|c| c.items().iter())
            .flat_map(move |i| i.items_within(start, end))
    }

    pub fn todos(&self) -> impl Iterator<Item = &CalTodo> {
        self.items().flat_map(|i| i.todos())
    }

    pub fn events(&self) -> impl Iterator<Item = &CalEvent> {
        self.items().flat_map(|i| i.events())
    }
}
