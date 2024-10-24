use chrono::DateTime;
use chrono_tz::Tz;

use crate::col::{CalItem, CalSource, Id, Occurrence};
use crate::objects::{CalDate, CalEvent, CalTodo};

#[derive(Default)]
pub struct CalStore {
    sources: Vec<CalSource>,
}

impl CalStore {
    pub fn add(&mut self, source: CalSource) {
        self.sources.push(source);
    }

    pub fn source(&self, id: Id) -> Option<&CalSource> {
        self.sources.iter().find(|s| s.id() == id)
    }

    pub fn sources(&self) -> &[CalSource] {
        &self.sources
    }

    pub fn items(&self) -> impl Iterator<Item = &CalItem> {
        self.sources.iter().flat_map(|c| c.items().iter())
    }

    pub fn occurrence_by_id<S: AsRef<str>>(
        &self,
        uid: S,
        rid: &CalDate,
        tz: &Tz,
    ) -> Option<Occurrence<'_>> {
        let uid_str = uid.as_ref();
        self.sources
            .iter()
            .find_map(|c| c.occurrence_by_id(uid_str, rid, tz))
    }

    pub fn occurrences_within(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    ) -> impl Iterator<Item = Occurrence<'_>> {
        self.sources
            .iter()
            .flat_map(|c| c.items().iter())
            .flat_map(move |i| i.occurrences_within(start, end))
    }

    pub fn todos(&self) -> impl Iterator<Item = &CalTodo> {
        self.items().flat_map(|i| i.todos())
    }

    pub fn events(&self) -> impl Iterator<Item = &CalEvent> {
        self.items().flat_map(|i| i.events())
    }
}
