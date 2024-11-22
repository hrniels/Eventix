use chrono::DateTime;
use chrono_tz::Tz;

use crate::col::{CalItem, CalSource, ColError, Id, Occurrence};
use crate::objects::{CalComponent, CalDate, CalEvent, CalTodo};

#[derive(Default, Debug, Eq, PartialEq)]
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

    pub fn source_mut(&mut self, id: Id) -> Option<&mut CalSource> {
        self.sources.iter_mut().find(|s| s.id() == id)
    }

    pub fn sources(&self) -> &[CalSource] {
        &self.sources
    }

    pub fn items(&self) -> impl Iterator<Item = &CalItem> {
        self.sources.iter().flat_map(|c| c.items().iter())
    }

    pub fn item_by_id<S: AsRef<str>>(&self, uid: S) -> Option<&CalItem> {
        let uid_str = uid.as_ref();
        self.sources.iter().find_map(|c| c.item_by_id(uid_str))
    }

    pub fn item_by_id_mut<S: AsRef<str>>(&mut self, uid: S) -> Option<&mut CalItem> {
        let uid_str = uid.as_ref();
        self.sources
            .iter_mut()
            .find_map(|c| c.item_by_id_mut(uid_str))
    }

    pub fn occurrence_by_id<S: AsRef<str>>(
        &self,
        uid: S,
        rid: Option<&CalDate>,
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
        self.filtered_occurrences_within(start, end, |_| true)
    }

    pub fn filtered_occurrences_within<F>(
        &self,
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        filter: F,
    ) -> impl Iterator<Item = Occurrence<'_>>
    where
        F: Fn(&CalComponent) -> bool + Clone,
    {
        self.sources
            .iter()
            .flat_map(|c| c.items().iter())
            .flat_map(move |i| i.filtered_occurrences_within(start, end, filter.clone()))
    }

    pub fn todos(&self) -> impl Iterator<Item = &CalTodo> {
        self.items().flat_map(|i| i.todos())
    }

    pub fn events(&self) -> impl Iterator<Item = &CalEvent> {
        self.items().flat_map(|i| i.events())
    }

    pub fn save(&self) -> Result<(), ColError> {
        for s in &self.sources {
            s.save()?;
        }
        Ok(())
    }
}
