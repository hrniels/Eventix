// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};

use anyhow::anyhow;

use crate::{
    CalendarSettings, CollectionSettings, EventixState, State, create_calendar_by_folder, sync,
};

use super::super::{Action, LockedAction};

/// Creates an enabled calendar in a writable collection and refreshes the local store.
///
/// Trims the display name and selects a sanitized folder unused by configured calendars or
/// existing files. Returns an error for empty names, missing or unsupported collections,
/// read-only backends, filesystem or backend failures, persistence failures or refresh failures.
/// Failures before persistence remove the new in-memory settings but may leave backend data.
#[derive(Debug)]
pub struct CreateCalendar {
    pub id: String,
    pub name: String,
}

#[async_trait::async_trait]
impl Action<()> for CreateCalendar {
    fn description(&self) -> &'static str {
        "Calendar added"
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<()> {
        let id = &self.id;
        let name = self.name.as_str();

        let mut state = state.lock().await;
        let name = name.trim();
        let (path, folder) = prepare_calendar_creation(&state, id, name).await?;
        let calendar_id = uuid::Uuid::new_v4().simple().to_string();
        state
            .settings_mut()
            .collections_mut()
            .get_mut(id)
            .expect("collection remains present while state is locked")
            .all_calendars_mut()
            .insert(calendar_id.clone(), new_calendar_settings(name, &folder));

        // Keep configured metadata available until remote discovery catches up.
        let result = async {
            tokio::fs::create_dir_all(path.join(&folder)).await?;
            create_calendar_by_folder(&mut state, id, &folder).await?;
            state.settings().write_to_file()?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if result.is_err() {
            state
                .settings_mut()
                .collections_mut()
                .get_mut(id)
                .expect("collection remains present while state is locked")
                .all_calendars_mut()
                .remove(&calendar_id);
        }
        result?;
        State::refresh_store(&mut state).await
    }
}

async fn prepare_calendar_creation(
    state: &State,
    id: &str,
    name: &str,
) -> anyhow::Result<(PathBuf, String)> {
    let locale = state.locale();
    if name.is_empty() {
        anyhow::bail!(locale.translate("error.calendar_name").to_owned());
    }
    let collection = state.settings().collections().get(id).ok_or_else(|| {
        anyhow!(
            locale
                .translate("Collection is no longer available")
                .to_owned()
        )
    })?;
    if collection.is_syncer_read_only() || !collection.syncer().supports_mkrm() {
        anyhow::bail!(locale.translate("Could not apply changes").to_owned());
    }
    let path = collection.path(state.xdg(), id);
    let folder = unique_folder_name(&path, collection, name).await?;
    Ok((path, folder))
}

fn new_calendar_settings(name: &str, folder: &str) -> CalendarSettings {
    let mut calendar = CalendarSettings::default();
    calendar.set_enabled(true);
    calendar.set_folder(folder.into());
    calendar.set_name(name.into());
    calendar.set_bgcolor("#555555".into());
    calendar.set_fgcolor("#ffffff".into());
    calendar
}

async fn unique_folder_name(
    path: &Path,
    collection: &CollectionSettings,
    name: &str,
) -> anyhow::Result<String> {
    let base = sanitize_folder_name(name);
    let mut folder = base.clone();
    let mut suffix = 2;
    while collection
        .all_calendars()
        .values()
        .any(|cal| cal.folder() == &folder)
        || tokio::fs::try_exists(path.join(&folder)).await?
    {
        folder = format!("{base}-{suffix}");
        suffix += 1;
    }
    Ok(folder)
}

fn sanitize_folder_name(name: &str) -> String {
    let mut folder = String::new();
    let mut last_was_sep = false;
    for ch in name.trim().chars() {
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() {
            folder.push(ch);
            last_was_sep = false;
        } else if matches!(ch, '-' | '_' | '.') && !folder.is_empty() && !last_was_sep {
            folder.push(ch);
            last_was_sep = matches!(ch, '-' | '_');
        } else if !folder.is_empty() && !last_was_sep {
            folder.push('-');
            last_was_sep = true;
        }
    }
    let folder = folder.trim_matches(['-', '_', '.']).to_owned();
    if folder.is_empty() {
        uuid::Uuid::new_v4().simple().to_string()
    } else {
        folder
    }
}

/// Deletes a collection remotely and removes it from in-memory settings.
///
/// The caller must persist settings and refresh the local store afterward.
#[derive(Debug)]
pub struct DeleteCollection {
    pub col_id: String,
}

#[async_trait::async_trait]
impl LockedAction<()> for DeleteCollection {
    fn description(&self) -> &'static str {
        "Collection removed"
    }

    async fn execute_locked(self: Box<Self>, state: &mut State) -> anyhow::Result<()> {
        let col_id = &self.col_id;

        sync::delete_collection(state, col_id).await?;

        state.settings_mut().collections_mut().remove(col_id);
        Ok(())
    }
}

/// Deletes a calendar from a collection both remotely and locally.
///
/// The caller must persist settings and refresh the local store afterward.
#[derive(Debug)]
pub struct DeleteCalendar {
    pub col_id: String,
    pub cal_id: String,
}

#[async_trait::async_trait]
impl LockedAction<()> for DeleteCalendar {
    fn description(&self) -> &'static str {
        "Calendar removed"
    }

    async fn execute_locked(self: Box<Self>, state: &mut State) -> anyhow::Result<()> {
        let col_id = &self.col_id;
        let cal_id = &self.cal_id;

        sync::delete_calendar(state, col_id, cal_id).await?;

        let col = state
            .settings_mut()
            .collections_mut()
            .get_mut(col_id)
            .ok_or_else(|| anyhow!("No collection '{}'", col_id))?;
        col.all_calendars_mut().remove(cal_id);
        Ok(())
    }
}

/// Deletes the remote calendar identified by `folder` and removes its local synced files.
#[derive(Debug)]
pub struct DeleteCalendarByFolder {
    pub col_id: String,
    pub folder: String,
}

#[async_trait::async_trait]
impl LockedAction<()> for DeleteCalendarByFolder {
    fn description(&self) -> &'static str {
        "Remote calendar deleted"
    }

    async fn execute_locked(self: Box<Self>, state: &mut State) -> anyhow::Result<()> {
        let col_id = &self.col_id;
        let folder = &self.folder;

        sync::delete_calendar_by_folder(state, col_id, folder).await
    }
}
