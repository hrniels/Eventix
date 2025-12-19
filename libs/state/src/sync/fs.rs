use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};

use crate::{
    State,
    sync::{SyncCalResult, Syncer},
};

pub struct FSSyncer {
    folder_id: HashMap<String, String>,
}

impl FSSyncer {
    pub fn new(folder_id: HashMap<String, String>) -> Self {
        Self { folder_id }
    }

    fn sync_folder(state: &mut State, id: &str) -> anyhow::Result<bool> {
        let Some(dir) = state.store_mut().directory_mut(&Arc::new(id.to_string())) else {
            // if we don't know this directory, the calendar is disabled - so, do nothing
            return Ok(false);
        };

        let mut seen_changes = false;
        seen_changes |= dir.rescan_for_additions()?;
        seen_changes |= dir.rescan_files()?;
        seen_changes |= dir.rescan_for_deletions();
        Ok(seen_changes)
    }
}

#[async_trait]
impl Syncer for FSSyncer {
    async fn discover(&self, _state: &mut State) -> anyhow::Result<SyncCalResult> {
        Ok(SyncCalResult::Success(false))
    }

    async fn sync_cal(
        &mut self,
        state: &mut State,
        cal_id: &String,
    ) -> anyhow::Result<SyncCalResult> {
        let seen_changes = Self::sync_folder(state, cal_id)?;
        Ok(SyncCalResult::Success(seen_changes))
    }

    async fn sync(&mut self, state: &mut State) -> anyhow::Result<SyncCalResult> {
        let mut seen_changes = false;
        for id in self.folder_id.values() {
            seen_changes |= Self::sync_folder(state, id)?;
        }

        Ok(SyncCalResult::Success(seen_changes))
    }

    async fn delete_cal(&mut self, _state: &mut State, _cal_id: &String) -> anyhow::Result<()> {
        // in this case we keep the data as there is no server side we could get it back from
        Ok(())
    }

    async fn delete(&mut self, _state: &mut State, _config: bool) -> anyhow::Result<()> {
        Ok(())
    }
}
