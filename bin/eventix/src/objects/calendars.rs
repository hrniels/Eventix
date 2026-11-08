// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_state::{CalendarSettings, State};

pub struct Calendar {
    pub id: String,
    pub col_id: String,
    pub name: String,
    pub enabled: bool,
    pub sync_error: bool,
    pub fgcolor: String,
    pub bgcolor: String,
    pub read_only: bool,
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
            .collections()
            .iter()
            .flat_map(|(col_id, col)| {
                col.calendars().filter_map(|(id, settings)| {
                    if filter(id, settings) {
                        Some(Calendar {
                            id: id.clone(),
                            col_id: col_id.clone(),
                            name: settings.name().clone(),
                            enabled: !state.misc().calendar_disabled(id),
                            sync_error: state.misc().collection_error(col_id).is_some(),
                            fgcolor: settings.fgcolor().clone(),
                            bgcolor: settings.bgcolor().clone(),
                            read_only: col.is_read_only(),
                        })
                    } else {
                        None
                    }
                })
            })
            .collect::<Vec<_>>();
        calendars.sort_by(|a, b| a.name.cmp(&b.name));
        Self(calendars)
    }
}
