// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use eventix_state::{State, SyncColResult, SyncResult};

/// Retains collection errors from sync responses for the current GTK session.
#[derive(Default)]
pub(crate) struct SyncErrors(HashMap<String, String>);

impl SyncErrors {
    pub fn update(&mut self, result: &SyncResult) {
        for (collection, outcome) in &result.collections {
            match outcome {
                SyncColResult::Error(error) => self.set_collection(collection, Some(error)),
                SyncColResult::Success(_) | SyncColResult::AuthFailed(_) => {
                    self.set_collection(collection, None);
                }
            }
        }
    }

    pub fn set_collection(&mut self, collection: &str, error: Option<&str>) {
        if let Some(error) = error {
            self.0.insert(collection.to_owned(), error.to_owned());
        } else {
            self.0.remove(collection);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CalendarVisibility {
    pub id: String,
    pub name: String,
    pub background: String,
    pub enabled: bool,
    pub sync_error: Option<String>,
}

/// Loads header calendars in name order, including those hidden from calendar views.
pub(crate) fn load(state: &State, errors: &SyncErrors) -> Vec<CalendarVisibility> {
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
                    sync_error: state.misc().has_calendar_error(id).then(|| {
                        errors.0.get(col_id).cloned().unwrap_or_else(|| {
                            state
                                .locale()
                                .translate("Calendar synchronization failed")
                                .to_string()
                        })
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

    use eventix_state::{
        CalendarSettings, CollectionSettings, State, SyncColResult, SyncResult, SyncerType,
    };

    use super::{SyncErrors, load};

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
        for id in ["office", "team"] {
            state.misc_mut().set_calendar_error(&id.into(), true);
        }
        let mut errors = SyncErrors::default();
        errors.update(&SyncResult {
            collections: [(
                "work".into(),
                SyncColResult::Error("Connection refused".into()),
            )]
            .into(),
            ..Default::default()
        });
        let calendars = load(&state, &errors);
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

        state
            .misc_mut()
            .set_calendar_error(&"personal".into(), true);
        assert!(load(&state, &errors)[1].sync_error.is_some());

        errors.update(&SyncResult::default());
        assert_eq!(
            load(&state, &errors)[0].sync_error.as_deref(),
            Some("Connection refused")
        );
        errors.update(&SyncResult {
            collections: [("home".into(), SyncColResult::Success(false))].into(),
            ..Default::default()
        });
        assert_eq!(
            load(&state, &errors)[0].sync_error.as_deref(),
            Some("Connection refused")
        );
        errors.update(&SyncResult {
            collections: [("work".into(), SyncColResult::Success(false))].into(),
            ..Default::default()
        });
        for id in ["office", "team"] {
            state.misc_mut().set_calendar_error(&id.into(), false);
        }
        assert_eq!(load(&state, &errors)[0].sync_error, None);
        assert_eq!(load(&state, &errors)[2].sync_error, None);
    }
}
