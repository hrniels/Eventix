use eventix_ical::col::CalDir;
use std::sync::Arc;

use crate::state::{CalendarSettings, State};

pub struct Calendar {
    pub id: Arc<String>,
    pub name: String,
    pub enabled: bool,
    pub sync_error: bool,
    pub fgcolor: String,
    pub bgcolor: String,
}

#[derive(Default)]
pub struct Calendars(pub Vec<Calendar>);

impl Calendars {
    pub fn new<F>(state: &State, filter: F) -> Self
    where
        F: Fn(&CalDir, &CalendarSettings) -> bool,
    {
        let mut calendars = state
            .store()
            .directories()
            .iter()
            .filter_map(|dir| {
                let settings = state.settings().calendar(dir.id()).unwrap();
                if filter(dir, settings) {
                    Some(Calendar {
                        id: dir.id().clone(),
                        name: dir.name().clone(),
                        enabled: !state.misc().calendar_disabled(dir.id()),
                        sync_error: state.misc().has_sync_error(dir.id()),
                        fgcolor: settings.fgcolor().clone(),
                        bgcolor: settings.bgcolor().clone(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        calendars.sort_by(|a, b| a.name.cmp(&b.name));
        Self(calendars)
    }
}
