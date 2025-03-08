use std::sync::Arc;

use ical::col::CalDir;

use crate::state::EventixState;

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
    pub async fn new_with_disabled(state: &EventixState) -> Self {
        let state = state.lock().await;
        let mut calendars = state
            .store()
            .directories()
            .iter()
            .map(|s| Calendar {
                id: s.id().clone(),
                name: s.name().clone(),
                enabled: !state.disabled_cals().contains(s.id()),
                fgcolor: s.props().get(&String::from("fgcolor")).unwrap().clone(),
                bgcolor: s.props().get(&String::from("bgcolor")).unwrap().clone(),
            })
            .collect::<Vec<_>>();
        calendars.sort_by(|a, b| a.name.cmp(&b.name));
        Self(calendars)
    }

    pub fn new<'a, I>(calendars: I) -> Self
    where
        I: Iterator<Item = &'a CalDir>,
    {
        let mut calendars = calendars
            .map(|s| Calendar {
                id: s.id().clone(),
                name: s.name().clone(),
                enabled: true,
                fgcolor: s.props().get(&String::from("fgcolor")).unwrap().clone(),
                bgcolor: s.props().get(&String::from("bgcolor")).unwrap().clone(),
            })
            .collect::<Vec<_>>();
        calendars.sort_by(|a, b| a.name.cmp(&b.name));
        Self(calendars)
    }
}
