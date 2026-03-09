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

use crate::misc::Misc;
use crate::settings::SyncerType;
use crate::sync::o365::O365;
use crate::sync::{fs::FSSyncer, vdirsyncer::VDirSyncer};
use crate::{CollectionSettings, State};

#[async_trait]
pub trait Syncer: Send {
    /// Discovers available calendars from the backend and updates state accordingly.
    async fn discover(&self, state: &mut State) -> anyhow::Result<SyncColResult>;

    /// Synchronises a single calendar identified by `cal_id`.
    #[allow(clippy::ptr_arg)]
    async fn sync_cal(
        &mut self,
        state: &mut State,
        cal_id: &String,
    ) -> anyhow::Result<SyncColResult>;

    /// Synchronises all calendars in this collection.
    async fn sync(&mut self, state: &mut State) -> anyhow::Result<SyncColResult>;

    /// Removes locally cached data for the calendar identified by `cal_id`.
    #[allow(clippy::ptr_arg)]
    async fn delete_cal(&mut self, state: &mut State, cal_id: &String) -> anyhow::Result<()>;

    /// Removes locally cached data for the entire collection.
    ///
    /// If `config` is `true`, configuration-related state is also removed.
    async fn delete(&mut self, state: &mut State, config: bool) -> anyhow::Result<()>;
}

/// Credentials used to authenticate with a remote syncer backend.
pub struct SyncerAuth {
    user: String,
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

impl SyncResult {
    fn new_from_single(col_id: String, cal_id: String, res: SyncColResult) -> Self {
        let changed = matches!(res, SyncColResult::Success(changed) if changed);
        let error = matches!(res, SyncColResult::Error(_));
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

struct CollectionSync {
    id: Arc<String>,
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

/// Runs calendar discovery for the collection identified by `col_id`.
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

/// Synchronises all calendars in the collection identified by `col_id`.
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

/// Reloads all calendars in the collection identified by `col_id` by deleting local state,
/// re-running discovery, and then syncing.
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

/// Deletes all local data for the collection identified by `col_id`, including configuration.
pub(crate) async fn delete_collection(state: &mut State, col_id: &String) -> anyhow::Result<()> {
    let mut cal_sync = syncer_for_collection(state, col_id, None).await?;
    cal_sync.syncer.delete(state, true).await
}

/// Reloads a single calendar by deleting its local state, re-running discovery, and syncing.
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

/// Deletes local cached data for the calendar identified by `cal_id` within `col_id`.
pub(crate) async fn delete_calendar(
    state: &mut State,
    col_id: &String,
    cal_id: &String,
) -> anyhow::Result<()> {
    let mut cal_sync = syncer_for_collection(state, col_id, None).await?;
    cal_sync.syncer.delete_cal(state, cal_id).await
}

/// Synchronises all calendars across all configured collections.
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

/// Returns the path to the sync log file for the collection identified by `col_id`.
pub fn log_file(xdg: &BaseDirectories, col_id: &String) -> PathBuf {
    let dir = xdg.get_data_file("vdirsyncer").unwrap();
    dir.join(format!("{}.log", col_id))
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
        state
            .misc_mut()
            .set_calendar_error(&cal_id, sync_error.is_some());
        sync_res.calendars.insert(cal_id, sync_error.is_some());
    }
}

async fn get_syncs(
    state: &mut State,
    auth_url: Option<&String>,
) -> anyhow::Result<Vec<CollectionSync>> {
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
) -> anyhow::Result<CollectionSync> {
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
    let log_path = log_file(xdg, id);
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
        SyncerType::VDirSyncer { url, read_only, .. } => Box::new(
            VDirSyncer::new(
                xdg,
                id.clone(),
                folder_id,
                url.clone(),
                *read_only,
                auth,
                log,
            )
            .await?,
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
                misc.collection_token(id).cloned(),
                log,
            )
            .await?,
        ),
        SyncerType::FileSystem { path: _ } => Box::new(FSSyncer::new(folder_id)),
    };

    Ok(CollectionSync {
        id: Arc::new(id.to_string()),
        syncer,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        misc::Misc,
        settings::{CalendarSettings, CollectionSettings, Settings, SyncerType},
    };
    use eventix_ical::col::CalStore;

    use super::{SyncColResult, SyncResult, sync_all};

    // --- helpers ---

    /// Creates an XDG `BaseDirectories` rooted at `root`.
    fn make_xdg(root: &std::path::Path) -> xdg::BaseDirectories {
        crate::with_test_xdg(&root.join("data"), &root.join("config"))
    }

    // --- SyncColResult / SyncResult ---

    #[test]
    fn new_from_single() {
        let res = SyncResult::new_from_single(
            "col".to_string(),
            "cal".to_string(),
            SyncColResult::Success(true),
        );

        assert!(res.changed, "changed must be true when Success(true)");
        assert_eq!(
            res.collections.get("col"),
            Some(&SyncColResult::Success(true))
        );
        // A successful sync sets the calendar error flag to false.
        assert_eq!(res.calendars.get("cal"), Some(&false));
    }

    // --- syncer_for_collection error path ---

    #[tokio::test]
    async fn discover_collection_unknown_id_returns_error() {
        let tmpdir = tempfile::tempdir().unwrap();
        // XDG must point somewhere valid; the error fires before any filesystem access since the
        // collection does not exist.
        let _xdg = make_xdg(tmpdir.path());

        let mut state =
            crate::State::new_for_test(CalStore::default(), Misc::new(PathBuf::default()));

        let err = super::discover_collection(&mut state, &"nonexistent".to_string(), None)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("nonexistent"),
            "error must mention the missing id: {err}"
        );
    }

    // --- sync_all ---

    #[tokio::test]
    async fn sync_all_empty_collections_returns_default_result() {
        // No XDG manipulation needed – no collections means no filesystem access.
        let mut state =
            crate::State::new_for_test(CalStore::default(), Misc::new(PathBuf::default()));

        let result = sync_all(&mut state, None).await.unwrap();

        assert!(!result.changed);
        assert!(result.collections.is_empty());
        assert!(result.calendars.is_empty());
    }

    #[tokio::test]
    async fn sync_all_multiple_collections_all_present_in_result() {
        let tmpdir = tempfile::tempdir().unwrap();
        let xdg = make_xdg(tmpdir.path());
        std::fs::create_dir_all(tmpdir.path().join("data/vdirsyncer")).unwrap();

        // Build state with two FS collections.
        let mut settings = Settings::new(PathBuf::default());
        for (col, cal) in [("colA", "calA"), ("colB", "calB")] {
            let mut col_settings = CollectionSettings::new(SyncerType::FileSystem {
                path: "/tmp".to_string(),
            });
            let mut cal_settings = CalendarSettings::default();
            cal_settings.set_enabled(true);
            cal_settings.set_folder("folder".to_string());
            col_settings
                .all_calendars_mut()
                .insert(cal.to_string(), cal_settings);
            settings
                .collections_mut()
                .insert(col.to_string(), col_settings);
        }
        // Use the XDG snapshot built above so that the log-file directory resolves correctly
        // regardless of what other tests do to the environment after this point.
        let mut state = crate::State::new_for_test_with_xdg(
            xdg,
            CalStore::default(),
            Misc::new(PathBuf::default()),
        );
        *state.settings_mut() = settings;

        let result = sync_all(&mut state, None).await.unwrap();

        assert!(result.collections.contains_key("colA"));
        assert!(result.collections.contains_key("colB"));
        assert_eq!(
            result.collections.get("colA"),
            Some(&SyncColResult::Success(false))
        );
        assert_eq!(
            result.collections.get("colB"),
            Some(&SyncColResult::Success(false))
        );
    }
}
