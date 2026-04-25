// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod fs;
mod o365;
mod vdirsyncer;

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use xdg::BaseDirectories;

use crate::settings::SyncerType;
use crate::sync::o365::O365;
use crate::sync::{fs::FSSyncer, vdirsyncer::VDirSyncer};
use crate::{CollectionSettings, State};

/// Defines the interface for a calendar synchronisation backend.
///
/// Each backend (filesystem, vdirsyncer, Microsoft 365) implements this trait to provide
/// discover, sync, and delete operations for its collections and individual calendars.
#[async_trait]
pub trait Syncer: Send {
    /// Discovers available calendars from the backend and updates state accordingly.
    async fn discover(&mut self) -> anyhow::Result<SyncColResult>;

    /// Synchronises a single calendar identified by `cal_id`.
    #[allow(clippy::ptr_arg)]
    async fn sync_cal(&mut self, cal_id: &String) -> anyhow::Result<SyncColResult>;

    /// Synchronises all calendars in this collection.
    async fn sync(&mut self) -> anyhow::Result<SyncColResult>;

    /// Removes locally cached data for the calendar identified by `cal_id`.
    #[allow(clippy::ptr_arg)]
    async fn delete_cal(&mut self, cal_id: &String) -> anyhow::Result<()>;

    /// Creates the remote calendar identified by `folder` and prepares local synced files.
    #[allow(clippy::ptr_arg)]
    async fn create_cal_by_folder(&mut self, folder: &String) -> anyhow::Result<()>;

    /// Deletes the remote calendar identified by `folder` and removes its local synced files.
    #[allow(clippy::ptr_arg)]
    async fn delete_cal_by_folder(&mut self, folder: &String) -> anyhow::Result<()>;

    /// Removes locally cached data for the entire collection.
    ///
    /// If `all` is `true`, the collection is deleted completely, including
    /// configuration-related state.
    async fn delete(&mut self, all: bool) -> anyhow::Result<()>;

    /// Finalizes a sync while holding the application state lock.
    fn finish(&mut self, _state: &mut State, _result: &mut SyncColResult) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Credentials used to authenticate with a remote syncer backend.
pub struct SyncerAuth {
    /// The account username (typically an email address).
    user: String,
    /// Shell command and arguments used to retrieve the account password at runtime.
    pw_cmd: Vec<String>,
}

/// The outcome of a single collection or calendar sync operation.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum SyncColResult {
    /// The operation succeeded; the boolean indicates whether any data changed.
    Success(bool),
    /// The operation failed with the given error message.
    Error(String),
    /// Authentication failed with the given URL to re-authenticate.
    AuthFailed(String),
}

/// The aggregated result of a sync operation across one or more collections.
#[derive(Debug, Default)]
pub struct SyncResult {
    /// Whether any calendar data changed during the operation.
    pub changed: bool,
    /// Per-collection outcome, keyed by collection ID.
    pub collections: HashMap<String, SyncColResult>,
    /// Per-calendar error flag, keyed by calendar ID; `true` if that calendar had an error.
    pub calendars: HashMap<String, bool>,
}

pub(crate) type SyncExecution = (
    CollectionSettings,
    Box<dyn Syncer + 'static>,
    anyhow::Result<SyncColResult>,
);

struct CollectionSync {
    snapshot: CollectionSettings,
    syncer: Box<dyn Syncer + 'static>,
}

/// Appends a timestamped line to the sync log, also emitting it at `DEBUG` level via tracing.
pub(crate) async fn log_line(log: &Arc<Mutex<File>>, name: &str, line: &str) -> anyhow::Result<()> {
    let buf = format!("{}: {}\n", name, line);
    tracing::debug!("{}", &buf[..buf.len() - 1]);
    // Skip the "name: " prefix so that the log file contains only the raw line, not the
    // collection name that is already implied by the file name.
    log.lock()
        .await
        .write_all(&buf.as_bytes()[name.len() + 2..])
        .await
        .context("log failed")
}

/// Deletes all local data for the collection identified by `col_id`, including configuration.
pub(crate) async fn delete_collection(state: &mut State, col_id: &String) -> anyhow::Result<()> {
    let (xdg, idx, col, token) = sync_snapshot(state, col_id)?;
    let mut cal_sync = get_sync_from_snapshot(xdg, idx, col_id.clone(), col, token, None).await?;
    cal_sync.syncer.delete(true).await?;

    let log_path = log_file(state.xdg(), col_id);
    match tokio::fs::remove_file(&log_path).await {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => {
            Err(anyhow!(e)).context(format!("Removing {} failed", log_path.display()))?;
        }
    }

    Ok(())
}

/// Deletes local cached data for the calendar identified by `cal_id` within `col_id`.
pub(crate) async fn delete_calendar(
    state: &mut State,
    col_id: &String,
    cal_id: &String,
) -> anyhow::Result<()> {
    let (xdg, idx, col, token) = sync_snapshot(state, col_id)?;
    let mut cal_sync = get_sync_from_snapshot(xdg, idx, col_id.clone(), col, token, None).await?;
    cal_sync.syncer.delete_cal(cal_id).await
}

/// Creates the remote calendar identified by `folder` within `col_id`.
pub async fn create_calendar_by_folder(
    state: &mut State,
    col_id: &String,
    folder: &String,
) -> anyhow::Result<()> {
    let (xdg, idx, col, token) = sync_snapshot(state, col_id)?;
    let mut cal_sync = get_sync_from_snapshot(xdg, idx, col_id.clone(), col, token, None).await?;
    cal_sync.syncer.create_cal_by_folder(folder).await
}

/// Deletes the remote calendar identified by `folder` and removes its local synced files.
pub(crate) async fn delete_calendar_by_folder(
    state: &mut State,
    col_id: &String,
    folder: &String,
) -> anyhow::Result<()> {
    let (xdg, idx, col, token) = sync_snapshot(state, col_id)?;
    let mut cal_sync = get_sync_from_snapshot(xdg, idx, col_id.clone(), col, token, None).await?;
    cal_sync.syncer.delete_cal_by_folder(folder).await
}

fn collection_index(state: &State, col_id: &String) -> anyhow::Result<usize> {
    state
        .settings()
        .collections()
        .keys()
        .enumerate()
        .find_map(|(idx, id)| (id == col_id).then_some(idx))
        .ok_or_else(|| anyhow!("No collection with id {}", col_id))
}

fn sync_snapshot(
    state: &State,
    col_id: &String,
) -> anyhow::Result<(
    Arc<BaseDirectories>,
    usize,
    CollectionSettings,
    Option<String>,
)> {
    Ok((
        state.xdg().clone().into(),
        collection_index(state, col_id)?,
        state
            .settings()
            .collections()
            .get(col_id)
            .cloned()
            .ok_or_else(|| anyhow!("No collection with id {}", col_id))?,
        state.misc().collection_token(col_id).cloned(),
    ))
}

/// Returns the path to the sync log file for the collection identified by `col_id`.
pub fn log_file(xdg: &BaseDirectories, col_id: &String) -> PathBuf {
    let dir = xdg.get_data_file("vdirsyncer").unwrap();
    dir.join(format!("{}.log", col_id))
}

pub(crate) async fn handle_sync_result(
    state: &mut State,
    col_id: &String,
    snapshot: &CollectionSettings,
    syncer: &mut Box<dyn Syncer + 'static>,
    res: anyhow::Result<SyncColResult>,
    sync_res: &mut SyncResult,
) {
    let mut res = match res {
        Ok(res) => res,
        Err(e) => SyncColResult::Error(e.to_string()),
    };

    if let Err(e) = syncer.finish(state, &mut res) {
        res = SyncColResult::Error(e.to_string());
    }

    match &res {
        SyncColResult::Success(cal_changed) => {
            sync_res.changed = *cal_changed;
            if *cal_changed {
                let _ = reload_collection_from_disk(state, col_id, Some(snapshot));
            }
        }
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
        state
            .misc_mut()
            .set_calendar_error(&cal_id, sync_error.is_some());
        sync_res.calendars.insert(cal_id, sync_error.is_some());
    }
}

async fn get_sync_from_snapshot(
    xdg: Arc<BaseDirectories>,
    idx: usize,
    id: String,
    col: CollectionSettings,
    token: Option<String>,
    auth_url: Option<String>,
) -> anyhow::Result<CollectionSync> {
    get_sync(xdg, idx, id, col, token, auth_url).await
}

async fn get_sync(
    xdg: Arc<BaseDirectories>,
    idx: usize,
    id: String,
    col: CollectionSettings,
    token: Option<String>,
    auth_url: Option<String>,
) -> anyhow::Result<CollectionSync> {
    // Phase 1: extract credentials from the syncer configuration, if any.
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

    // Phase 2: build a folder-name → calendar-id map used by the syncer to route sync events.
    let folder_id = col
        .calendars()
        .map(|(id, settings)| (settings.folder().clone(), id.clone()))
        .collect::<HashMap<_, _>>();
    // Phase 3: set up the per-collection log file (clearing any previous run) and construct the
    // appropriate Syncer implementation.
    let log_path = log_file(&xdg, &id);
    let log_dir = log_path.parent().unwrap();
    if !log_dir.exists() {
        tokio::fs::create_dir(log_dir).await?;
    }
    if log_path.exists() {
        tokio::fs::remove_file(&log_path).await.ok();
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .await?;
    let log = Arc::new(Mutex::new(log));

    let syncer: Box<dyn Syncer> = match col.syncer() {
        SyncerType::VDirSyncer {
            url,
            read_only,
            time_span,
            ..
        } => Box::new(
            VDirSyncer::new(
                &xdg,
                id.clone(),
                folder_id,
                url.clone(),
                *read_only,
                auth,
                time_span,
                log,
            )
            .await?,
        ),
        SyncerType::O365 {
            read_only,
            time_span,
            ..
        } => Box::new(
            O365::new(
                &xdg,
                idx,
                id.clone(),
                folder_id,
                *read_only,
                auth.unwrap(),
                auth_url.as_ref(),
                token,
                time_span,
                log,
            )
            .await?,
        ),
        SyncerType::FileSystem { path: _ } => Box::new(FSSyncer::new(folder_id.into_values())),
    };

    Ok(CollectionSync {
        snapshot: col,
        syncer,
    })
}

pub(crate) fn reload_collection_from_disk(
    state: &mut State,
    col_id: &String,
    previous: Option<&CollectionSettings>,
) -> anyhow::Result<()> {
    let current = state.settings().collections().get(col_id).cloned();
    let mut remove_ids = previous
        .into_iter()
        .flat_map(|col| col.all_calendars().keys().cloned())
        .collect::<Vec<_>>();
    if let Some(col) = &current {
        remove_ids.extend(col.all_calendars().keys().cloned());
    }

    state
        .store_mut()
        .retain(|dir| !remove_ids.iter().any(|id| dir.id().as_ref() == id));

    let Some(col) = current else {
        return Ok(());
    };

    let local_tz = *state.timezone();
    let mut dirs = vec![];
    for (cal_id, cal) in col.calendars() {
        let dir = State::load_calendar(state.xdg(), col_id, &col, cal_id, cal, &local_tz)?;
        dirs.push(dir);
    }
    for dir in dirs {
        state.store_mut().add(dir);
    }
    Ok(())
}

pub(crate) async fn run_sync_from_snapshot(
    xdg: Arc<BaseDirectories>,
    idx: usize,
    id: String,
    col: CollectionSettings,
    token: Option<String>,
    auth_url: Option<String>,
) -> anyhow::Result<SyncExecution> {
    let mut cal_sync = get_sync(xdg, idx, id, col, token, auth_url).await?;
    let snapshot = cal_sync.snapshot.clone();
    let res = cal_sync.syncer.sync().await;
    Ok((snapshot, cal_sync.syncer, res))
}

pub(crate) async fn run_discover_from_snapshot(
    xdg: Arc<BaseDirectories>,
    idx: usize,
    id: String,
    col: CollectionSettings,
    token: Option<String>,
    auth_url: Option<String>,
) -> anyhow::Result<SyncExecution> {
    let mut cal_sync = get_sync(xdg, idx, id, col, token, auth_url).await?;
    let snapshot = cal_sync.snapshot.clone();
    let res = cal_sync.syncer.discover().await;
    Ok((snapshot, cal_sync.syncer, res))
}

pub(crate) async fn run_reload_collection_from_snapshot(
    xdg: Arc<BaseDirectories>,
    idx: usize,
    id: String,
    col: CollectionSettings,
    token: Option<String>,
    auth_url: Option<String>,
) -> anyhow::Result<SyncExecution> {
    let mut cal_sync = get_sync(xdg, idx, id, col, token, auth_url).await?;
    let snapshot = cal_sync.snapshot.clone();
    cal_sync.syncer.delete(false).await?;
    cal_sync.syncer.discover().await?;
    let res = cal_sync.syncer.sync().await;
    Ok((snapshot, cal_sync.syncer, res))
}

pub(crate) async fn run_reload_calendar_from_snapshot(
    xdg: Arc<BaseDirectories>,
    idx: usize,
    id: String,
    col: CollectionSettings,
    token: Option<String>,
    auth_url: Option<String>,
    cal_id: &String,
) -> anyhow::Result<SyncExecution> {
    let mut cal_sync = get_sync(xdg, idx, id, col, token, auth_url).await?;
    let snapshot = cal_sync.snapshot.clone();
    cal_sync.syncer.delete_cal(cal_id).await?;
    cal_sync.syncer.discover().await?;
    let res = cal_sync.syncer.sync_cal(cal_id).await;
    Ok((snapshot, cal_sync.syncer, res))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use eventix_ical::col::CalStore;

    use crate::{
        misc::Misc,
        settings::{CalendarSettings, CollectionSettings, Settings, SyncerType},
    };

    use super::{SyncColResult, SyncResult, sync_snapshot};

    #[test]
    fn sync_result_shape_for_single_success() {
        let mut collections = std::collections::HashMap::new();
        collections.insert("col".to_string(), SyncColResult::Success(true));
        let mut calendars = std::collections::HashMap::new();
        calendars.insert("cal".to_string(), false);
        let res = SyncResult {
            changed: true,
            collections,
            calendars,
        };

        assert!(res.changed);
        assert_eq!(
            res.collections.get("col"),
            Some(&SyncColResult::Success(true))
        );
        assert_eq!(res.calendars.get("cal"), Some(&false));
    }

    #[test]
    fn sync_snapshot_unknown_collection_returns_error() {
        let state = crate::State::new_for_test(CalStore::default(), Misc::new(PathBuf::default()));
        let err = sync_snapshot(&state, &"missing".to_string()).unwrap_err();
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn sync_snapshot_captures_collection_and_token() {
        let mut settings = Settings::new(PathBuf::default());
        let mut col = CollectionSettings::new(SyncerType::FileSystem {
            path: "/tmp/cals".to_string(),
        });
        let mut cal = CalendarSettings::default();
        cal.set_enabled(true);
        cal.set_folder("folder".to_string());
        col.all_calendars_mut().insert("cal1".to_string(), cal);
        settings
            .collections_mut()
            .insert("col1".to_string(), col.clone());

        let mut state =
            crate::State::new_for_test(CalStore::default(), Misc::new(PathBuf::default()));
        *state.settings_mut() = settings;
        state
            .misc_mut()
            .set_collection_token(&"col1".to_string(), "tok".to_string());

        let (_xdg, idx, snapshot_col, token) = sync_snapshot(&state, &"col1".to_string()).unwrap();

        assert_eq!(idx, 0);
        assert_eq!(snapshot_col, col);
        assert_eq!(token, Some("tok".to_string()));
    }
}
