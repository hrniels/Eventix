// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Owned calendar choices, completions, and defaults for event and task editors.

use std::sync::Arc;

use chrono::{Duration, NaiveDateTime, Timelike, Utc};
use eventix_ical::objects::CalCompType;
use eventix_locale::Locale;
use eventix_state::State;

/// A writable calendar available for creating items.
pub struct EditorCalendar {
    pub id: String,
    pub name: String,
    pub background: String,
    pub personal: bool,
}

/// Owned choices and initial defaults for an item, independent of UI widgets.
pub struct EditorOptions {
    pub kind: CalCompType,
    pub calendars: Vec<EditorCalendar>,
    pub last_calendar: Option<String>,
    pub locations: Vec<String>,
    pub contacts: Vec<String>,
    pub start: NaiveDateTime,
    pub locale: Arc<dyn Locale + Send + Sync>,
}

impl EditorOptions {
    /// Loads calendar choices, completions, and defaults for a new event.
    ///
    /// Includes enabled event calendars from writable collections and sorts choices by name
    /// and ID. The initial start is the next whole hour in the configured timezone; returns
    /// an error if that date and time cannot be represented.
    pub fn load(state: &State) -> anyhow::Result<Self> {
        Self::load_for(state, CalCompType::Event)
    }

    /// Loads writable calendars and defaults for the requested component type.
    pub fn load_for(state: &State, kind: CalCompType) -> anyhow::Result<Self> {
        let mut calendars = Vec::new();
        for collection in state.settings().collections().values() {
            if collection.is_read_only() {
                continue;
            }
            for (id, settings) in collection.calendars() {
                if settings.types().contains(&kind) {
                    calendars.push(EditorCalendar {
                        id: id.clone(),
                        name: settings.name().clone(),
                        background: settings.bgcolor().clone(),
                        personal: matches!(
                            settings.alarms(),
                            eventix_state::CalendarAlarmType::Personal { .. }
                        ),
                    });
                }
            }
        }
        calendars.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        let now = Utc::now().with_timezone(state.timezone());
        let start = now
            .with_minute(0)
            .and_then(|date| date.with_second(0))
            .and_then(|date| date.with_nanosecond(0))
            .and_then(|date| date.checked_add_signed(Duration::hours(1)))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    state
                        .locale()
                        .translate("error.valid_date_time")
                        .to_string()
                )
            })?
            .naive_local();
        let mut locations: Vec<_> = state.store().locations().into_iter().collect();
        locations.sort();
        let mut contacts: Vec<_> = state
            .store()
            .contacts()
            .into_iter()
            .map(|(address, name)| {
                if address == name {
                    address
                } else {
                    format!("{name} <{address}>")
                }
            })
            .collect();
        contacts.sort();
        Ok(Self {
            kind,
            calendars,
            locations,
            contacts,
            start,
            locale: state.locale(),
            last_calendar: state.misc().last_calendar(kind).cloned(),
        })
    }
}
