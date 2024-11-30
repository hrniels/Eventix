use std::sync::Arc;

use ical::col::CalSource;

use crate::state::State;

pub struct Calendar {
    pub id: Arc<String>,
    pub name: String,
    pub enabled: bool,
}

#[derive(Default)]
pub struct Calendars(pub Vec<Calendar>);

impl Calendars {
    pub async fn new_with_disabled(state: &State) -> Self {
        let (store, disabled) = state.acquire_store_and_disabled().await;
        let mut calendars = store
            .sources()
            .iter()
            .map(|s| Calendar {
                id: s.id().clone(),
                name: s.name().clone(),
                enabled: !disabled.contains(s.id()),
            })
            .collect::<Vec<_>>();
        calendars.sort_by(|a, b| a.name.cmp(&b.name));
        Self(calendars)
    }

    pub fn new<'a, I>(calendars: I) -> Self
    where
        I: Iterator<Item = &'a CalSource>,
    {
        let mut calendars = calendars
            .map(|s| Calendar {
                id: s.id().clone(),
                name: s.name().clone(),
                enabled: true,
            })
            .collect::<Vec<_>>();
        calendars.sort_by(|a, b| a.name.cmp(&b.name));
        Self(calendars)
    }
}
