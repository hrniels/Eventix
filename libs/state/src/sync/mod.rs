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

pub struct SyncerAuth {
    user: String,
    pw_cmd: Vec<String>,
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

pub async fn sync_all(
    state: EventixState,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let mut tasks = Vec::new();
    for mut cmd in get_syncs(state.clone(), auth_url).await? {
        let state_clone = state.clone();
        tasks.push((
            cmd.id.clone(),
            tokio::spawn(async move { cmd.syncer.sync(&cmd.id, state_clone).await }),
        ));
    }

    let mut changed = false;
    let mut calendars = HashMap::new();
    for (id, handle) in tasks {
        let res = match handle.await? {
            Ok(res) => res,
            Err(e) => SyncCalResult::Error(e.to_string()),
        };

        match &res {
            SyncCalResult::Success(cal_changed) => changed |= cal_changed,
            SyncCalResult::Error(msg) => tracing::error!("{}: failed with {}", id, msg),
            SyncCalResult::AuthFailed(_) => tracing::error!("{}: auth failed", id),
        }

        // extract error message
        let sync_error = match &res {
            SyncCalResult::Error(msg) => Some(msg.clone()),
            _ => None,
        };

        // set the error for all calendars within this collection
        let ids = state
            .lock()
            .await
            .settings()
            .collections()
            .get(&*id)
            .unwrap()
            .calendars()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for cal_id in ids {
            state
                .lock()
                .await
                .misc_mut()
                .set_sync_error(&cal_id, sync_error.clone());
            calendars.insert(cal_id, res.clone());
        }
    }

    Ok(SyncResult { changed, calendars })
}

async fn get_syncs(
    state: EventixState,
    auth_url: Option<&String>,
) -> anyhow::Result<Vec<CalendarSync>> {
    let state = state.lock().await;
    let mut res = vec![];
    for (idx, (id, col)) in state.settings().collections().iter().enumerate() {
        let auth = match col.syncer() {
            SyncerType::VDirSyncer {
                password_cmd: Some(password_cmd),
                ..
            }
            | SyncerType::O365 { password_cmd, .. } => {
                let user = col.email().map(|e| e.address().clone());
                Some(SyncerAuth {
                    user: user.unwrap(),
                    pw_cmd: password_cmd.clone(),
                })
            }
            _ => None,
        };

        let folder_id = col
            .calendars()
            .map(|(id, settings)| (settings.folder().clone(), id.clone()))
            .collect::<HashMap<_, _>>();

        let syncer: Box<dyn Syncer> = match col.syncer() {
            SyncerType::VDirSyncer { url, read_only, .. } => Box::new(
                VDirSyncer::new(
                    state.xdg(),
                    id.clone(),
                    folder_id,
                    url.clone(),
                    *read_only,
                    auth,
                )
                .await?,
            ),
            SyncerType::O365 { read_only, .. } => Box::new(
                O365::new(
                    state.xdg(),
                    idx,
                    id.clone(),
                    folder_id,
                    *read_only,
                    auth.unwrap(),
                    auth_url,
                    state.misc().calendar_token(id).cloned(),
                )
                .await?,
            ),
            SyncerType::FileSystem { path: _ } => Box::new(FSSyncer::new(folder_id)),
        };

        res.push(CalendarSync {
            id: Arc::new(id.to_string()),
            syncer,
        });
    }
    Ok(res)
}
