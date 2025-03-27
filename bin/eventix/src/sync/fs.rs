use anyhow::anyhow;
use async_trait::async_trait;
use std::sync::Arc;

use crate::{state::EventixState, sync::Syncer};

pub struct FSSyncer;

#[async_trait]
impl Syncer for FSSyncer {
    async fn sync(&mut self, cal: &Arc<String>, state: EventixState) -> anyhow::Result<bool> {
        let mut state = state.lock().await;

        let last_reload = state.last_reload();
        let dir = state
            .store_mut()
            .directory_mut(&cal)
            .ok_or_else(|| anyhow!("directory '{}' does not exist", cal))?;

        let mut seen_changes = false;
        seen_changes |= dir.rescan_for_additions()?;
        seen_changes |= dir.rescan_for_updates(last_reload)?;
        seen_changes |= dir.rescan_for_deletions();

        Ok(seen_changes)
    }
}
