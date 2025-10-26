use anyhow::anyhow;
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::MutexGuard;

use crate::{
    EventixState, State,
    sync::{SyncCalResult, Syncer},
};

pub struct FSSyncer {
    folder_id: HashMap<String, String>,
}

impl FSSyncer {
    pub fn new(folder_id: HashMap<String, String>) -> Self {
        Self { folder_id }
    }

    fn sync_folder(state: &mut MutexGuard<'_, State>, id: &String) -> anyhow::Result<bool> {
        let dir = state
            .store_mut()
            .directory_mut(&Arc::new(id.clone()))
            .ok_or_else(|| anyhow!("directory '{}' does not exist", id))?;

        let mut seen_changes = false;
        seen_changes |= dir.rescan_for_additions()?;
        seen_changes |= dir.rescan_files()?;
        seen_changes |= dir.rescan_for_deletions();
        Ok(seen_changes)
    }
}

#[async_trait]
impl Syncer for FSSyncer {
    async fn discover(&self, _state: EventixState) -> anyhow::Result<SyncCalResult> {
        Ok(SyncCalResult::Success(false))
    }

    async fn sync_cal(
        &mut self,
        state: EventixState,
        cal_id: &String,
    ) -> anyhow::Result<SyncCalResult> {
        let mut state = state.lock().await;
        let seen_changes = Self::sync_folder(&mut state, cal_id)?;
        Ok(SyncCalResult::Success(seen_changes))
    }

    async fn sync(&mut self, state: EventixState) -> anyhow::Result<SyncCalResult> {
        let mut state = state.lock().await;

        let mut seen_changes = false;
        for id in self.folder_id.values() {
            seen_changes |= Self::sync_folder(&mut state, id)?;
        }

        Ok(SyncCalResult::Success(seen_changes))
    }

    async fn delete_cal(&mut self, _state: EventixState, _cal_id: &String) -> anyhow::Result<()> {
        Err(anyhow!("Delete is not supported"))
    }

    async fn delete(&mut self, _state: EventixState) -> anyhow::Result<()> {
        Err(anyhow!("Delete is not supported"))
    }
}
