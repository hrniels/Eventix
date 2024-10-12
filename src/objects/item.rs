use std::path::PathBuf;

use super::Id;

pub struct CalItem {
    id: Id,
    path: PathBuf,
    item: icalendar::Calendar,
}

impl CalItem {
    pub fn new(path: PathBuf, item: icalendar::Calendar) -> Self {
        Self {
            id: super::generate_id(),
            path,
            item,
        }
    }

    pub fn id(&self) -> Id {
        self.id
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn item(&self) -> &icalendar::Calendar {
        &self.item
    }

    pub fn todos(&self) -> impl Iterator<Item = &icalendar::Todo> {
        self.item
            .components
            .iter()
            .filter(|&c| matches!(c, icalendar::CalendarComponent::Todo(_)))
            .map(|t| t.as_todo().unwrap())
    }

    pub fn events(&self) -> impl Iterator<Item = &icalendar::Event> {
        self.item
            .components
            .iter()
            .filter(|&c| matches!(c, icalendar::CalendarComponent::Event(_)))
            .map(|e| e.as_event().unwrap())
    }
}
