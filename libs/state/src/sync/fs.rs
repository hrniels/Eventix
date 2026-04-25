// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::State;
use crate::sync::{SyncColResult, Syncer};
use async_trait::async_trait;
use std::collections::HashMap;

pub struct FSSyncer {
    changed: HashMap<String, bool>,
}

/// A [`Syncer`] implementation that reads calendar data directly from the local filesystem.
///
/// Unlike network-backed syncers, `FSSyncer` performs no remote synchronisation; it rescans
/// local directories on each sync to detect additions, modifications, and deletions.
impl FSSyncer {
    /// Creates a new `FSSyncer` with the given mapping from folder names to calendar IDs.
    pub fn new(ids: impl IntoIterator<Item = String>) -> Self {
        Self {
            changed: ids.into_iter().map(|id| (id, false)).collect(),
        }
    }
}

#[async_trait]
impl Syncer for FSSyncer {
    async fn discover(&mut self) -> anyhow::Result<SyncColResult> {
        Ok(SyncColResult::Success(false))
    }

    async fn sync_cal(&mut self, cal_id: &String) -> anyhow::Result<SyncColResult> {
        let Some(changed) = self.changed.get_mut(cal_id) else {
            return Ok(SyncColResult::Success(false));
        };
        *changed = true;
        Ok(SyncColResult::Success(false))
    }

    async fn sync(&mut self) -> anyhow::Result<SyncColResult> {
        for changed in self.changed.values_mut() {
            *changed = true;
        }
        Ok(SyncColResult::Success(false))
    }

    async fn delete_cal(&mut self, _cal_id: &String) -> anyhow::Result<()> {
        // in this case we keep the data as there is no server side we could get it back from
        Ok(())
    }

    async fn create_cal_by_folder(&mut self, _folder: &String) -> anyhow::Result<()> {
        Ok(())
    }

    async fn delete_cal_by_folder(&mut self, _folder: &String) -> anyhow::Result<()> {
        Ok(())
    }

    async fn delete(&mut self, _all: bool) -> anyhow::Result<()> {
        Ok(())
    }

    fn finish(&mut self, state: &mut State, result: &mut SyncColResult) -> anyhow::Result<()> {
        let local_tz = *state.timezone();
        let mut collection_changed = false;

        for (cal_id, should_sync) in &mut self.changed {
            if !*should_sync {
                continue;
            }

            let dir = state
                .store_mut()
                .try_directory_mut(&cal_id.clone().into())?;

            let mut changed = false;
            changed |= dir.rescan_for_additions(&local_tz)?;
            changed |= dir.rescan_files(&local_tz)?;
            changed |= dir.rescan_for_deletions();
            *should_sync = changed;
            collection_changed |= changed;
        }

        *result = SyncColResult::Success(collection_changed);

        Ok(())
    }
}
