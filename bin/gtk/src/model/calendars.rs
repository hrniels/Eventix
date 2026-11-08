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
    pub sync_error: Option<String>,
}

/// Loads header calendars in name order, including those hidden from calendar views.
pub(crate) fn load(state: &State) -> Vec<CalendarVisibility> {
    let mut calendars: Vec<_> = state
        .settings()
        .collections()
        .iter()
        .flat_map(|(col_id, collection)| {
            collection
                .calendars()
                .map(move |(id, settings)| CalendarVisibility {
                    id: id.clone(),
                    name: settings.name().clone(),
                    background: settings.bgcolor().clone(),
                    enabled: !state.misc().calendar_disabled(id),
                    sync_error: state.misc().collection_error(col_id).map(|message| {
                        if message.is_empty() {
                            state
                                .locale()
                                .translate("Calendar synchronization failed")
                                .to_string()
                        } else {
                            message.to_owned()
                        }
                    }),
                })
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};

    use super::load;

    #[test]
    fn collection_errors_apply_to_all_header_calendars_but_not_other_collections() {
        let dir = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(dir.path(), dir.path()));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        for (collection_id, calendars) in [
            ("work", vec![("team", "Team"), ("office", "Office")]),
            ("home", vec![("personal", "Personal")]),
        ] {
            let mut collection = CollectionSettings::new(SyncerType::FileSystem {
                path: dir.path().to_string_lossy().into_owned(),
            });
            for (id, name) in calendars {
                let mut calendar = CalendarSettings::default();
                calendar.set_name(name.into());
                calendar.set_enabled(true);
                collection.all_calendars_mut().insert(id.into(), calendar);
            }
            state
                .settings_mut()
                .collections_mut()
                .insert(collection_id.into(), collection);
        }
        state.misc_mut().toggle_calendar(&"office".into());
        state
            .misc_mut()
            .set_collection_error("work", Some("Connection refused"));
        state.settings().write_to_file().unwrap();
        state.misc().write_to_file().unwrap();
        // Recreate state as on startup, without any in-memory sync response cache.
        let mut state =
            State::new_with_timezone(state.xdg().clone().into(), Some(chrono_tz::UTC)).unwrap();
        let calendars = load(&state);
        assert_eq!(
            calendars
                .iter()
                .map(|calendar| calendar.id.as_str())
                .collect::<Vec<_>>(),
            ["office", "personal", "team"]
        );
        assert!(!calendars[0].enabled);
        assert_eq!(
            calendars[0].sync_error.as_deref(),
            Some("Connection refused")
        );
        assert_eq!(calendars[1].sync_error, None);
        assert_eq!(
            calendars[2].sync_error.as_deref(),
            Some("Connection refused")
        );

        state.misc_mut().set_collection_error("home", Some(""));
        assert_eq!(
            load(&state)[1].sync_error.as_deref(),
            Some("Calendar synchronization failed")
        );
        state.misc_mut().set_collection_error("home", None);
        assert_eq!(
            load(&state)[0].sync_error.as_deref(),
            Some("Connection refused")
        );
        state.misc_mut().set_collection_error("work", None);
        assert_eq!(load(&state)[0].sync_error, None);
        assert_eq!(load(&state)[2].sync_error, None);
    }
}
