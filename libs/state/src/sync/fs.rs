// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};

use crate::{
    State,
    sync::{SyncColResult, Syncer},
};

/// A [`Syncer`] implementation that reads calendar data directly from the local filesystem.
///
/// Unlike network-backed syncers, `FSSyncer` performs no remote synchronisation; it rescans
/// local directories on each sync to detect additions, modifications, and deletions.
pub struct FSSyncer {
    folder_id: HashMap<String, String>,
}

impl FSSyncer {
    /// Creates a new `FSSyncer` with the given mapping from folder names to calendar IDs.
    pub fn new(folder_id: HashMap<String, String>) -> Self {
        Self { folder_id }
    }

    fn sync_folder(state: &mut State, id: &str) -> anyhow::Result<bool> {
        let local_tz = *state.timezone();
        let Some(dir) = state.store_mut().directory_mut(&Arc::new(id.to_string())) else {
            // if we don't know this directory, the calendar is disabled - so, do nothing
            return Ok(false);
        };

        let mut seen_changes = false;
        seen_changes |= dir.rescan_for_additions(&local_tz)?;
        seen_changes |= dir.rescan_files(&local_tz)?;
        seen_changes |= dir.rescan_for_deletions();
        Ok(seen_changes)
    }
}

#[async_trait]
impl Syncer for FSSyncer {
    async fn discover(&self, _state: &mut State) -> anyhow::Result<SyncColResult> {
        Ok(SyncColResult::Success(false))
    }

    async fn sync_cal(
        &mut self,
        state: &mut State,
        cal_id: &String,
    ) -> anyhow::Result<SyncColResult> {
        let seen_changes = Self::sync_folder(state, cal_id)?;
        Ok(SyncColResult::Success(seen_changes))
    }

    async fn sync(&mut self, state: &mut State) -> anyhow::Result<SyncColResult> {
        let mut seen_changes = false;
        for id in self.folder_id.values() {
            seen_changes |= Self::sync_folder(state, id)?;
        }

        Ok(SyncColResult::Success(seen_changes))
    }

    async fn delete_cal(&mut self, _state: &mut State, _cal_id: &String) -> anyhow::Result<()> {
        // in this case we keep the data as there is no server side we could get it back from
        Ok(())
    }

    async fn create_cal_by_folder(
        &mut self,
        _state: &mut State,
        _folder: &String,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn delete_cal_by_folder(
        &mut self,
        _state: &mut State,
        _folder: &String,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn delete(&mut self, _state: &mut State, _all: bool) -> anyhow::Result<()> {
        Ok(())
    }
}
