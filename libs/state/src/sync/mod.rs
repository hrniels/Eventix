mod fs;
mod o365;
mod vdirsyncer;

use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::EventixState;
use crate::settings::SyncerType;
use crate::sync::o365::O365;
use crate::sync::{fs::FSSyncer, vdirsyncer::VDirSyncer};

#[async_trait]
pub trait Syncer: Send {
    async fn sync(
        &mut self,
        cal: &Arc<String>,
        state: EventixState,
    ) -> anyhow::Result<SyncCalResult>;
}

#[derive(Clone, Debug, Serialize)]
pub enum SyncCalResult {
    Success(bool),
    Error(String),
    AuthFailed(String),
}

#[derive(Default)]
pub struct SyncResult {
    pub changed: bool,
    pub calendars: HashMap<String, SyncCalResult>,
}

struct CalendarSync {
    id: Arc<String>,
    syncer: Box<dyn Syncer + 'static>,
}

impl CalendarSync {}

pub async fn sync_all(state: EventixState, auth_url: Option<&String>) -> SyncResult {
    let mut tasks = Vec::new();
    for mut cmd in get_syncs(state.clone(), auth_url).await.unwrap() {
        let state_clone = state.clone();
        tasks.push((
            cmd.id.clone(),
            tokio::spawn(async move { cmd.syncer.sync(&cmd.id, state_clone).await }),
        ));
    }

    let mut changed = false;
    let mut calendars = HashMap::new();
    for (id, handle) in tasks {
        let res = match handle.await.unwrap() {
            Ok(res) => res,
            Err(e) => SyncCalResult::Error(e.to_string()),
        };
        calendars.insert((*id).clone(), res.clone());

        let sync_error = match &res {
            SyncCalResult::Error(msg) => Some(msg.clone()),
            _ => None,
        };
        state
            .lock()
            .await
            .misc_mut()
            .set_sync_error(&id, sync_error);

        match res {
            SyncCalResult::Success(cal_changed) => changed |= cal_changed,
            SyncCalResult::Error(msg) => tracing::error!("{}: failed with {}", id, msg),
            SyncCalResult::AuthFailed(_) => tracing::error!("{}: auth failed", id),
        }
    }

    SyncResult { changed, calendars }
}

async fn get_syncs(
    state: EventixState,
    auth_url: Option<&String>,
) -> anyhow::Result<Vec<CalendarSync>> {
    let state = state.lock().await;
    let mut res = vec![];
    for (id, settings) in state.settings().calendars().iter() {
        let syncer: Box<dyn Syncer> = match settings.syncer() {
            SyncerType::VDirSyncer { name, local_name } => {
                Box::new(VDirSyncer::new(name.clone(), local_name.clone()))
            }
            SyncerType::O365 {
                name,
                local_name,
                port,
            } => Box::new(O365::new(
                name.clone(),
                local_name.clone(),
                *port,
                settings.email().unwrap().address().clone(),
                auth_url,
                state.misc().calendar_token(id).cloned(),
            )?),
            SyncerType::FileSystem => Box::new(FSSyncer),
        };
        res.push(CalendarSync {
            id: Arc::new(id.to_string()),
            syncer,
        });
    }
    Ok(res)
}
