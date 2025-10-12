use anyhow::anyhow;
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};

use crate::{
    EventixState,
    sync::{SyncCalResult, Syncer},
};

pub struct FSSyncer {
    folder_id: HashMap<String, String>,
}

impl FSSyncer {
    pub fn new(folder_id: HashMap<String, String>) -> Self {
        Self { folder_id }
    }
}

#[async_trait]
impl Syncer for FSSyncer {
    async fn sync(
        &mut self,
        _col: &Arc<String>,
        state: EventixState,
    ) -> anyhow::Result<SyncCalResult> {
        let mut state = state.lock().await;

        let mut seen_changes = false;
        for id in self.folder_id.values() {
            let dir = state
                .store_mut()
                .directory_mut(&Arc::new(id.clone()))
                .ok_or_else(|| anyhow!("directory '{}' does not exist", id))?;

            seen_changes |= dir.rescan_for_additions()?;
            seen_changes |= dir.rescan_files()?;
            seen_changes |= dir.rescan_for_deletions();
        }

        Ok(SyncCalResult::Success(seen_changes))
    }
}
