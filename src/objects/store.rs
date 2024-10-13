use chrono::{DateTime, Utc};

use super::{CalItem, CalSource};

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
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> impl Iterator<Item = &icalendar::CalendarComponent> {
        self.sources
            .iter()
            .map(|c| c.items().iter())
            .flatten()
            .map(move |i| i.items_within(start, end))
            .flatten()
    }

    pub fn todos(&self) -> impl Iterator<Item = &icalendar::Todo> {
        self.items().map(|i| i.todos()).flatten()
    }

    pub fn events(&self) -> impl Iterator<Item = &icalendar::Event> {
        self.items().map(|i| i.events()).flatten()
    }
}
