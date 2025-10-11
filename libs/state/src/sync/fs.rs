use anyhow::anyhow;
use async_trait::async_trait;
use std::sync::Arc;

use crate::{
    EventixState,
    sync::{SyncCalResult, Syncer},
};

pub struct FSSyncer;

#[async_trait]
impl Syncer for FSSyncer {
    async fn sync(
        &mut self,
        cal: &Arc<String>,
        state: EventixState,
    ) -> anyhow::Result<SyncCalResult> {
        let mut state = state.lock().await;

        let dir = state
            .store_mut()
            .directory_mut(cal)
            .ok_or_else(|| anyhow!("directory '{}' does not exist", cal))?;

        let mut seen_changes = false;
        seen_changes |= dir.rescan_for_additions()?;
        seen_changes |= dir.rescan_files()?;
        seen_changes |= dir.rescan_for_deletions();

        Ok(SyncCalResult::Success(seen_changes))
    }
}
