// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use eventix_state::{CalendarSettings, EventixState, State, SyncCancellation, SyncerType};

#[derive(Clone, Debug)]
pub(crate) enum Operation {
    Enable(bool),
    Save(Box<CalendarSettings>),
    Reload,
    Delete,
}

#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub collection: String,
    pub calendar: String,
    pub folder: String,
    pub configured: bool,
    pub operation: Operation,
}

impl Request {
    /// Returns the translation key for this calendar action's outcome.
    pub(crate) fn description(&self) -> &'static str {
        match self.operation {
            Operation::Enable(true) => "Calendar enabled",
            Operation::Enable(false) => "Calendar disabled",
            Operation::Save(_) if self.configured => "Calendar settings updated",
            Operation::Save(_) => "Calendar added",
            Operation::Reload => "Calendar reloaded",
            Operation::Delete if self.configured => "Calendar removed",
            Operation::Delete => "Remote calendar deleted",
        }
    }
}

/// Applies calendar management changes using the same state operations as the HTML UI.
pub(crate) async fn apply(state: &EventixState, request: &Request) -> anyhow::Result<()> {
    if matches!(request.operation, Operation::Reload) {
        return reload(state, request).await;
    }

    let mut state = state.lock().await;
    let unavailable = state
        .locale()
        .translate("Calendar is no longer available")
        .to_owned();
    let collection = state
        .settings_mut()
        .collections_mut()
        .get_mut(&request.collection)
        .ok_or_else(|| anyhow!(unavailable.clone()))?;
    let previous = collection.clone();
    if request.configured && !collection.all_calendars().contains_key(&request.calendar) {
        anyhow::bail!(unavailable);
    }

    match &request.operation {
        Operation::Enable(enabled) => {
            let calendar = collection
                .all_calendars_mut()
                .get_mut(&request.calendar)
                .ok_or_else(|| anyhow!(unavailable.clone()))?;
            calendar.set_enabled(*enabled);
        }
        Operation::Save(settings) => {
            let mut settings = settings.as_ref().clone();
            let existing = request
                .configured
                .then(|| collection.all_calendars().get(&request.calendar))
                .flatten();
            if !matches!(collection.syncer(), SyncerType::FileSystem { .. }) {
                settings.set_folder(existing.map_or_else(
                    || request.folder.clone(),
                    |calendar| calendar.folder().clone(),
                ));
            }
            settings.set_enabled(existing.is_none_or(CalendarSettings::enabled));
            if !request.configured
                && collection
                    .all_calendars()
                    .values()
                    .any(|calendar| calendar.folder() == settings.folder())
            {
                anyhow::bail!(unavailable);
            }
            let id = if request.configured {
                request.calendar.clone()
            } else {
                uuid::Uuid::new_v4().simple().to_string()
            };
            collection.all_calendars_mut().insert(id, settings);
        }
        Operation::Delete => {
            if request.configured {
                State::delete_calendar(&mut state, &request.collection, &request.calendar).await?;
            } else {
                State::delete_calendar_by_folder(&mut state, &request.collection, &request.folder)
                    .await?;
            }
        }
        Operation::Reload => unreachable!(),
    }

    if let Err(error) = state.settings().write_to_file() {
        // Settings-only changes can be rolled back; deleted files cannot be restored.
        if !matches!(request.operation, Operation::Delete) {
            state
                .settings_mut()
                .collections_mut()
                .insert(request.collection.clone(), previous);
        }
        State::refresh_store(&mut state).await?;
        return Err(error).context(
            state
                .locale()
                .translate("Could not apply changes")
                .to_owned(),
        );
    }
    State::refresh_store(&mut state).await
}

async fn reload(state: &EventixState, request: &Request) -> anyhow::Result<()> {
    let locale = {
        let state = state.lock().await;
        if !request.configured
            || state
                .settings()
                .collections()
                .get(&request.collection)
                .and_then(|collection| collection.all_calendars().get(&request.calendar))
                .is_none()
        {
            anyhow::bail!(
                state
                    .locale()
                    .translate("Calendar is no longer available")
                    .to_owned()
            );
        }
        state.locale()
    };
    let result = State::reload_calendar(
        state,
        &request.collection,
        &request.calendar,
        None,
        &SyncCancellation::default(),
    )
    .await?;
    for outcome in result.collections.values() {
        match outcome {
            eventix_state::SyncColResult::Error(error) => anyhow::bail!(error.clone()),
            eventix_state::SyncColResult::AuthFailed(_) => {
                anyhow::bail!(
                    locale
                        .translate("Authentication failed. Please try again.")
                        .to_owned()
                );
            }
            eventix_state::SyncColResult::Success(_) => {}
        }
    }
    if result.calendars.values().any(|failed| *failed) {
        anyhow::bail!(
            locale
                .translate("Calendar synchronization failed")
                .to_owned()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use eventix_ical::objects::{CalAction, CalAlarm, CalCompType, CalRelated, CalTrigger};
    use eventix_state::{CalendarAlarmType, CollectionSettings};

    use super::{CalendarSettings, EventixState, Operation, Request, State, SyncerType, apply};

    fn state(path: &Path) -> EventixState {
        let xdg = Arc::new(eventix_state::with_test_xdg(path, path));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: path.join("calendars").to_string_lossy().into_owned(),
        });
        std::fs::create_dir_all(path.join("calendars/team")).unwrap();
        let mut calendar = CalendarSettings::default();
        calendar.set_name("Team".into());
        calendar.set_folder("team".into());
        calendar.set_enabled(true);
        collection
            .all_calendars_mut()
            .insert("team-id".into(), calendar);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        Arc::new(tokio::sync::Mutex::new(state))
    }

    fn request(operation: Operation) -> Request {
        Request {
            collection: "local".into(),
            calendar: "team-id".into(),
            folder: "team".into(),
            configured: true,
            operation,
        }
    }

    #[tokio::test]
    async fn enablement_is_persisted_and_independent_of_header_visibility() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        for enabled in [false, true] {
            apply(&state, &request(Operation::Enable(enabled)))
                .await
                .unwrap();
            let state = state.lock().await;
            let settings = &state.settings().collections()["local"].all_calendars()["team-id"];
            assert_eq!(settings.enabled(), enabled);
            assert!(!state.misc().calendar_disabled(&"team-id".into()));
            let loaded =
                State::new_with_timezone(Arc::new(state.xdg().clone()), Some(chrono_tz::UTC))
                    .unwrap();
            assert_eq!(
                loaded.settings().collections()["local"].all_calendars()["team-id"].enabled(),
                enabled
            );
        }
    }

    #[tokio::test]
    async fn save_preserves_enablement_and_updates_all_editable_settings() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        apply(&state, &request(Operation::Enable(false)))
            .await
            .unwrap();
        let mut settings = CalendarSettings::default();
        settings.set_name("Updated".into());
        settings.set_folder("other".into());
        settings.set_bgcolor("#123456".into());
        settings.set_fgcolor("#ffffff".into());
        settings.set_types(vec![CalCompType::Todo]);
        let alarms = CalendarAlarmType::Personal {
            default: Some(CalAlarm::new(
                CalAction::Display,
                CalTrigger::Relative {
                    duration: chrono::Duration::minutes(-30).into(),
                    related: CalRelated::End,
                },
            )),
        };
        settings.set_alarms(alarms.clone());
        apply(&state, &request(Operation::Save(Box::new(settings))))
            .await
            .unwrap();
        let state = state.lock().await;
        let saved = &state.settings().collections()["local"].all_calendars()["team-id"];
        assert_eq!(saved.name(), "Updated");
        assert_eq!(saved.folder(), "other");
        assert_eq!(saved.bgcolor(), "#123456");
        assert_eq!(saved.fgcolor(), "#ffffff");
        assert_eq!(saved.types(), &vec![CalCompType::Todo]);
        assert_eq!(saved.alarms(), &alarms);
        assert!(!saved.enabled());
    }

    #[tokio::test]
    async fn adding_discovered_calendar_enables_it_and_rejects_stale_adds() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let mut settings = CalendarSettings::default();
        settings.set_folder("discovered".into());
        settings.set_name("Discovered".into());
        let mut request = request(Operation::Save(Box::new(settings)));
        request.calendar = "discovered".into();
        request.configured = false;
        apply(&state, &request).await.unwrap();
        {
            let state = state.lock().await;
            let (id, added) = state.settings().collections()["local"]
                .all_calendars()
                .iter()
                .find(|(_, calendar)| calendar.folder() == "discovered")
                .unwrap();
            assert_ne!(id, "discovered");
            assert!(added.enabled());
        }
        assert!(apply(&state, &request).await.is_err());
    }

    #[tokio::test]
    async fn discovered_folders_with_the_same_name_get_distinct_calendar_ids() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        {
            let mut state = state.lock().await;
            let mut second = state.settings().collections()["local"].clone();
            second.all_calendars_mut().clear();
            state
                .settings_mut()
                .collections_mut()
                .insert("second".into(), second);
        }
        let mut settings = CalendarSettings::default();
        settings.set_folder("discovered".into());
        let mut request = request(Operation::Save(Box::new(settings)));
        request.calendar = "discovered".into();
        request.configured = false;
        apply(&state, &request).await.unwrap();
        request.collection = "second".into();
        apply(&state, &request).await.unwrap();
        let state = state.lock().await;
        let id = |collection: &str| {
            state.settings().collections()[collection]
                .all_calendars()
                .iter()
                .find(|(_, calendar)| calendar.folder() == "discovered")
                .unwrap()
                .0
                .clone()
        };
        assert_ne!(id("local"), id("second"));
    }

    #[tokio::test]
    async fn remote_save_keeps_current_folder_even_when_the_draft_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        state
            .lock()
            .await
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .set_syncer(SyncerType::VDirSyncer {
                email: eventix_state::EmailAccount::new("User".into(), "user@example.com".into()),
                url: "https://example.invalid".into(),
                read_only: true,
                username: None,
                password: None,
                time_span: Default::default(),
            });
        let mut settings = CalendarSettings::default();
        settings.set_name("Updated".into());
        settings.set_folder("not-the-remote-folder".into());
        let mut request = request(Operation::Save(Box::new(settings)));
        request.folder = "stale-folder".into();
        apply(&state, &request).await.unwrap();
        let state = state.lock().await;
        let saved = &state.settings().collections()["local"].all_calendars()["team-id"];
        assert_eq!(saved.name(), "Updated");
        assert_eq!(saved.folder(), "team");
    }

    #[tokio::test]
    async fn local_reload_and_delete_keep_source_files_and_remove_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let file = dir.path().join("calendars/team/event.ics");
        std::fs::write(&file, "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:test\r\nDTSTART:20261031T100000Z\r\nSUMMARY:Test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").unwrap();
        State::refresh_store(&mut *state.lock().await)
            .await
            .unwrap();
        apply(&state, &request(Operation::Reload)).await.unwrap();
        assert!(file.exists());
        apply(&state, &request(Operation::Delete)).await.unwrap();
        assert!(file.exists());
        assert!(
            state.lock().await.settings().collections()["local"]
                .all_calendars()
                .is_empty()
        );
        assert!(apply(&state, &request(Operation::Delete)).await.is_err());
    }

    #[tokio::test]
    async fn failed_settings_write_restores_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let path = state
            .lock()
            .await
            .xdg()
            .place_config_file("settings.toml")
            .unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(
            apply(&state, &request(Operation::Enable(false)))
                .await
                .is_err()
        );
        assert!(
            state.lock().await.settings().collections()["local"].all_calendars()["team-id"]
                .enabled()
        );
    }
}
