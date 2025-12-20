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
use crate::{CollectionSettings, State};

#[async_trait]
pub trait Syncer: Send {
    async fn discover(&self, state: &mut State) -> anyhow::Result<SyncColResult>;

    #[allow(clippy::ptr_arg)]
    async fn sync_cal(
        &mut self,
        state: &mut State,
        cal_id: &String,
    ) -> anyhow::Result<SyncColResult>;

    async fn sync(&mut self, state: &mut State) -> anyhow::Result<SyncColResult>;

    #[allow(clippy::ptr_arg)]
    async fn delete_cal(&mut self, state: &mut State, cal_id: &String) -> anyhow::Result<()>;

    async fn delete(&mut self, state: &mut State, config: bool) -> anyhow::Result<()>;
}

pub struct SyncerAuth {
    user: String,
    pw_cmd: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub enum SyncColResult {
    Success(bool),
    Error(String),
    AuthFailed(String),
}

#[derive(Default)]
pub struct SyncResult {
    pub changed: bool,
    pub collections: HashMap<String, SyncColResult>,
    pub calendars: HashMap<String, Option<String>>,
}

impl SyncResult {
    fn new_from_single(col_id: String, cal_id: String, res: SyncColResult) -> Self {
        let changed = matches!(res, SyncColResult::Success(changed) if changed);
        let error = if let SyncColResult::Error(msg) = &res {
            Some(msg.clone())
        } else {
            None
        };
        let mut collections = HashMap::new();
        collections.insert(col_id, res);
        let mut calendars = HashMap::new();
        calendars.insert(cal_id, error);
        Self {
            changed,
            collections,
            calendars,
        }
    }
}

struct CalendarSync {
    id: Arc<String>,
    syncer: Box<dyn Syncer + 'static>,
}

pub(crate) async fn discover_collection(
    state: &mut State,
    col_id: &String,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let cal_sync = syncer_for_collection(state, col_id, auth_url).await?;

    let mut sync_res = SyncResult::default();
    let res = cal_sync.syncer.discover(state).await;
    handle_sync_result(state, col_id, res, &mut sync_res).await;
    Ok(sync_res)
}

pub(crate) async fn sync_collection(
    state: &mut State,
    col_id: &String,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let mut cal_sync = syncer_for_collection(state, col_id, auth_url).await?;

    let mut sync_res = SyncResult::default();
    let res = cal_sync.syncer.sync(state).await;
    handle_sync_result(state, col_id, res, &mut sync_res).await;

    Ok(sync_res)
}

pub(crate) async fn reload_collection(
    state: &mut State,
    col_id: &String,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let mut cal_sync = syncer_for_collection(state, col_id, auth_url).await?;

    cal_sync.syncer.delete(state, false).await?;
    cal_sync.syncer.discover(state).await?;

    let mut sync_res = SyncResult::default();
    let res = cal_sync.syncer.sync(state).await;
    handle_sync_result(state, col_id, res, &mut sync_res).await;

    Ok(sync_res)
}

pub(crate) async fn delete_collection(state: &mut State, col_id: &String) -> anyhow::Result<()> {
    let mut cal_sync = syncer_for_collection(state, col_id, None).await?;
    cal_sync.syncer.delete(state, true).await
}

pub(crate) async fn reload_calendar(
    state: &mut State,
    col_id: &String,
    cal_id: &String,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let mut cal_sync = syncer_for_collection(state, col_id, auth_url).await?;

    cal_sync.syncer.delete_cal(state, cal_id).await?;
    cal_sync.syncer.discover(state).await?;
    let res = cal_sync.syncer.sync_cal(state, cal_id).await?;

    Ok(SyncResult::new_from_single(
        col_id.to_string(),
        cal_id.to_string(),
        res,
    ))
}

pub(crate) async fn delete_calendar(
    state: &mut State,
    col_id: &String,
    cal_id: &String,
) -> anyhow::Result<()> {
    let mut cal_sync = syncer_for_collection(state, col_id, None).await?;
    cal_sync.syncer.delete_cal(state, cal_id).await
}

pub(crate) async fn sync_all(
    state: &mut State,
    auth_url: Option<&String>,
) -> anyhow::Result<SyncResult> {
    let mut sync_res = SyncResult::default();

    for mut cmd in get_syncs(state, auth_url).await? {
        let res = cmd.syncer.sync(state).await;
        handle_sync_result(state, &cmd.id, res, &mut sync_res).await;
    }

    Ok(sync_res)
}

async fn handle_sync_result(
    state: &mut State,
    col_id: &String,
    res: anyhow::Result<SyncColResult>,
    sync_res: &mut SyncResult,
) {
    let res = match res {
        Ok(res) => res,
        Err(e) => SyncColResult::Error(e.to_string()),
    };

    match &res {
        SyncColResult::Success(cal_changed) => sync_res.changed = *cal_changed,
        SyncColResult::Error(msg) => tracing::error!("{}: failed with {}", col_id, msg),
        SyncColResult::AuthFailed(_) => tracing::error!("{}: auth failed", col_id),
    }

    // extract error message
    let sync_error = match &res {
        SyncColResult::Error(msg) => Some(msg.clone()),
        _ => None,
    };

    sync_res.collections.insert(col_id.clone(), res.clone());

    // set the error for all calendars within this collection
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
        sync_res.calendars.insert(cal_id, sync_error.clone());
    }
}

async fn get_syncs(
    state: &mut State,
    auth_url: Option<&String>,
) -> anyhow::Result<Vec<CalendarSync>> {
    let mut res = vec![];
    for (idx, (id, col)) in state.settings().collections().iter().enumerate() {
        let cal_sync = get_sync(state.xdg(), idx, id, col, state.misc(), auth_url).await?;
        res.push(cal_sync);
    }
    Ok(res)
}

async fn syncer_for_collection(
    state: &State,
    col_id: &String,
    auth_url: Option<&String>,
) -> anyhow::Result<CalendarSync> {
    let col = state
        .settings()
        .collections()
        .get(col_id)
        .ok_or_else(|| anyhow!("No collection with id {}", col_id))?;

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
