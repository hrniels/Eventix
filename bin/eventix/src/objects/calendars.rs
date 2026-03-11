// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_state::{CalendarSettings, State};

pub struct Calendar {
    pub id: String,
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
        F: Fn(&String, &CalendarSettings) -> bool,
    {
        let mut calendars = state
            .settings()
            .calendars()
            .filter_map(|(id, settings)| {
                if filter(id, settings) {
                    Some(Calendar {
                        id: id.clone(),
                        name: settings.name().clone(),
                        enabled: !state.misc().calendar_disabled(id),
                        sync_error: state.misc().has_calendar_error(id),
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
