// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_state::State;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CalendarVisibility {
    pub id: String,
    pub name: String,
    pub background: String,
    pub enabled: bool,
}

/// Loads header calendars in name order, including those hidden from calendar views.
pub(crate) fn load(state: &State) -> Vec<CalendarVisibility> {
    let mut calendars: Vec<_> = state
        .settings()
        .calendars()
        .map(|(id, settings)| CalendarVisibility {
            id: id.clone(),
            name: settings.name().clone(),
            background: settings.bgcolor().clone(),
            enabled: !state.misc().calendar_disabled(id),
        })
        .collect();
    calendars.sort_by(|a, b| a.name.cmp(&b.name));
    calendars
}

/// Persists a calendar visibility toggle, restoring its previous state if writing fails.
pub(crate) fn toggle(state: &mut State, id: &String) -> anyhow::Result<()> {
    if state.settings().calendar(id).is_none() {
        anyhow::bail!(
            state
                .locale()
                .translate("Calendar is no longer available")
                .to_string()
        );
    }
    let misc = state.misc_mut();
    misc.toggle_calendar(id);
    if let Err(error) = misc.write_to_file() {
        misc.toggle_calendar(id);
        return Err(error);
    }
    Ok(())
}
