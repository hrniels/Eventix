use std::sync::Arc;

use ical::col::CalDir;

use crate::{settings::CalendarSettings, state::State};

pub struct Calendar {
    pub id: Arc<String>,
    pub name: String,
    pub enabled: bool,
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
                        enabled: !settings.disabled(),
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
