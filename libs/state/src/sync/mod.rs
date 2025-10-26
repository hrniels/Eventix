mod fs;
mod o365;
mod vdirsyncer;

use anyhow::anyhow;
use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use xdg::BaseDirectories;

use crate::misc::Misc;
use crate::settings::SyncerType;
use crate::sync::o365::O365;
use crate::sync::{fs::FSSyncer, vdirsyncer::VDirSyncer};
use crate::{CollectionSettings, EventixState};

#[async_trait]
pub trait Syncer: Send {
    async fn discover(&self, state: EventixState) -> anyhow::Result<SyncCalResult>;

    #[allow(clippy::ptr_arg)]
    async fn sync_cal(
        &mut self,
        state: EventixState,
        cal_id: &String,
    ) -> anyhow::Result<SyncCalResult>;

    async fn sync(&mut self, state: EventixState) -> anyhow::Result<SyncCalResult>;

    #[allow(clippy::ptr_arg)]
    async fn delete_cal(&mut self, state: EventixState, cal_id: &String) -> anyhow::Result<()>;

    async fn delete(&mut self, state: EventixState) -> anyhow::Result<()>;
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

impl SyncResult {
    fn new_from_single(cal_id: String, res: SyncCalResult) -> Self {
        let changed = matches!(res, SyncCalResult::Success(changed) if changed);
        let mut calendars = HashMap::new();
        calendars.insert(cal_id, res);
        Self { changed, calendars }
    }
}

struct CalendarSync {
    id: Arc<String>,
    syncer: Box<dyn Syncer + 'static>,
}

pub(crate) async fn discover_collection(
    state: EventixState,
    col_id: &String,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let cal_sync = syncer_for_collection(&state, col_id, auth_url).await?;

    let mut sync_res = SyncResult::default();
    let res = cal_sync.syncer.discover(state.clone()).await;
    handle_sync_result(&state, col_id, res, &mut sync_res).await;
    Ok(sync_res)
}

pub(crate) async fn sync_collection(
    state: EventixState,
    col_id: &String,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let mut cal_sync = syncer_for_collection(&state, col_id, auth_url).await?;

    let mut sync_res = SyncResult::default();
    let res = cal_sync.syncer.sync(state.clone()).await;
    handle_sync_result(&state, col_id, res, &mut sync_res).await;

    Ok(sync_res)
}

pub(crate) async fn reload_collection(
    state: EventixState,
    col_id: &String,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let mut cal_sync = syncer_for_collection(&state, col_id, auth_url).await?;

    cal_sync.syncer.delete(state.clone()).await?;
    cal_sync.syncer.discover(state.clone()).await?;

    let mut sync_res = SyncResult::default();
    let res = cal_sync.syncer.sync(state.clone()).await;
    handle_sync_result(&state, col_id, res, &mut sync_res).await;

    Ok(sync_res)
}

pub(crate) async fn reload_calendar(
    state: EventixState,
    col_id: &String,
    cal_id: &String,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let mut cal_sync = syncer_for_collection(&state, col_id, auth_url).await?;

    cal_sync.syncer.delete_cal(state.clone(), cal_id).await?;
    cal_sync.syncer.discover(state.clone()).await?;
    let res = cal_sync.syncer.sync_cal(state, cal_id).await?;

    Ok(SyncResult::new_from_single(cal_id.to_string(), res))
}

pub(crate) async fn delete_calendar(
    state: EventixState,
    col_id: &String,
    cal_id: &String,
) -> anyhow::Result<()> {
    let mut cal_sync = syncer_for_collection(&state, col_id, None).await?;
    cal_sync.syncer.delete_cal(state.clone(), cal_id).await
}

pub(crate) async fn sync_all(
    state: EventixState,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let mut tasks = Vec::new();
    for mut cmd in get_syncs(state.clone(), auth_url).await? {
        let state_clone = state.clone();
        tasks.push((
            cmd.id.clone(),
            tokio::spawn(async move { cmd.syncer.sync(state_clone).await }),
        ));
    }

    let mut sync_res = SyncResult::default();

    for (id, handle) in tasks {
        let res = handle.await?;
        handle_sync_result(&state, &id, res, &mut sync_res).await;
    }

    Ok(sync_res)
}

async fn handle_sync_result(
    state: &EventixState,
    col_id: &String,
    res: anyhow::Result<SyncCalResult>,
    sync_res: &mut SyncResult,
) {
    let res = match res {
        Ok(res) => res,
        Err(e) => SyncCalResult::Error(e.to_string()),
    };

    match &res {
        SyncCalResult::Success(cal_changed) => sync_res.changed = *cal_changed,
        SyncCalResult::Error(msg) => tracing::error!("{}: failed with {}", col_id, msg),
        SyncCalResult::AuthFailed(_) => tracing::error!("{}: auth failed", col_id),
    }

    // extract error message
    let sync_error = match &res {
        SyncCalResult::Error(msg) => Some(msg.clone()),
        _ => None,
    };

    // set the error for all calendars within this collection
    let mut state = state.lock().await;
    let ids = state
        .settings()
        .collections()
        .get(col_id)
        .unwrap()
        .calendars()
        .map(|(cal_id, _)| cal_id.clone())
        .collect::<Vec<_>>();
    for cal_id in ids {
        state.misc_mut().set_sync_error(&cal_id, sync_error.clone());
        sync_res.calendars.insert(cal_id, res.clone());
    }
}

async fn get_syncs(
    state: EventixState,
    auth_url: Option<&String>,
) -> anyhow::Result<Vec<CalendarSync>> {
    let state = state.lock().await;
    let mut res = vec![];
    for (idx, (id, col)) in state.settings().collections().iter().enumerate() {
        let cal_sync = get_sync(state.xdg(), idx, id, col, state.misc(), auth_url).await?;
        res.push(cal_sync);
    }
    Ok(res)
}

async fn syncer_for_collection(
    state: &EventixState,
    col_id: &String,
    auth_url: Option<&String>,
) -> anyhow::Result<CalendarSync> {
    let state = state.lock().await;

    let col = state
        .settings()
        .collections()
        .get(col_id)
        .ok_or_else(|| anyhow!("No collection with id {}", col_id))?;

    // TODO we need to think about parallelism here (what if sync and discover run in parallel?)
    get_sync(state.xdg(), 0, col_id, col, state.misc(), auth_url).await
}

async fn get_sync(
    xdg: &BaseDirectories,
    idx: usize,
    id: &String,
    col: &CollectionSettings,
    misc: &Misc,
    auth_url: Option<&String>,
) -> anyhow::Result<CalendarSync> {
    let auth = match col.syncer() {
        SyncerType::VDirSyncer {
            username: Some(username),
            password_cmd: Some(password_cmd),
            ..
        } => Some(SyncerAuth {
            user: username.clone(),
            pw_cmd: password_cmd.clone(),
        }),
        SyncerType::O365 { password_cmd, .. } => {
            let user = col.email().map(|e| e.address());
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
            VDirSyncer::new(xdg, id.clone(), folder_id, url.clone(), *read_only, auth).await?,
        ),
        SyncerType::O365 { read_only, .. } => Box::new(
            O365::new(
                xdg,
                idx,
                id.clone(),
                folder_id,
                *read_only,
                auth.unwrap(),
                auth_url,
                misc.calendar_token(id).cloned(),
            )
            .await?,
        ),
        SyncerType::FileSystem { path: _ } => Box::new(FSSyncer::new(folder_id)),
    };

    Ok(CalendarSync {
        id: Arc::new(id.to_string()),
        syncer,
    })
}
