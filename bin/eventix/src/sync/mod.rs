mod fs;
mod vdirsyncer;

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::state::EventixState;
use crate::sync::{fs::FSSyncer, vdirsyncer::VDirSyncer};

#[async_trait]
pub trait Syncer: Send {
    async fn sync(&mut self, cal: &Arc<String>, state: EventixState) -> anyhow::Result<bool>;
}

#[derive(Default)]
pub struct SyncResult {
    pub changed: bool,
    pub calendars: HashMap<String, bool>,
}

struct CalendarSync {
    id: Arc<String>,
    syncer: Box<dyn Syncer + 'static>,
}

impl CalendarSync {}

pub async fn sync_all(state: EventixState) -> SyncResult {
    let mut tasks = Vec::new();
    for mut cmd in get_syncs(state.clone()).await {
        let state_clone = state.clone();
        tasks.push((
            cmd.id.clone(),
            tokio::spawn(async move { cmd.syncer.sync(&cmd.id, state_clone).await }),
        ));
    }

    let mut changed = false;
    let mut calendars = HashMap::new();
    for (id, handle) in tasks {
        let res = handle.await.unwrap();
        calendars.insert((*id).clone(), res.is_ok());
        state
            .lock()
            .await
            .misc_mut()
            .set_sync_error(&id, res.is_err());
        match res {
            Ok(res) => changed |= res,
            Err(e) => tracing::error!("{}: failed with {}", id, e),
        }
    }

    SyncResult { changed, calendars }
}

async fn get_syncs(state: EventixState) -> Vec<CalendarSync> {
    let state = state.lock().await;
    state
        .settings()
        .calendars()
        .iter()
        .map(|(id, settings)| {
            let syncer: Box<dyn Syncer> = match settings.syncer() {
                crate::state::Syncer::VDirSyncer { cmd, local_name } => {
                    Box::new(VDirSyncer::new(cmd.clone(), local_name.clone()))
                }
                crate::state::Syncer::FileSystem => Box::new(FSSyncer),
            };
            CalendarSync {
                id: Arc::new(id.to_string()),
                syncer,
            }
        })
        .collect()
}
