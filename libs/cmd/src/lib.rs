// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inter-process command dispatch for eventix.
//!
//! This crate provides a client/server protocol for sending [`Request`]s to a running eventix
//! daemon and receiving [`Response`]s. Communication happens over a Unix domain socket whose
//! path is derived from the XDG runtime directory.
//!
//! # Server side
//!
//! Call [`handle_commands`] inside the daemon process. It binds the socket, then loops
//! indefinitely, deserialising each incoming request, executing it against the shared
//! [`EventixState`], and writing the response back.
//!
//! # Client side
//!
//! - [`send`] — connects to the daemon socket and forwards a request, returning an error if no
//!   daemon is running.

use anyhow::Context;
use chrono::Local;
use eventix_ical::{col::CalFile, objects::EventLike};
use eventix_state::EventixState;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{fs::File, path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    task,
};
use tracing::{debug, error};
use xdg::BaseDirectories;

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    /// Import an iCalendar file into a named calendar.
    Import(ImportOptions),
    /// Search for a calendar item by UID.
    Search(String),
    /// Query the number of tasks due today and overdue.
    TaskStatus,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Response {
    /// The request completed successfully without a return value.
    Success,
    /// The response for a [`Request::Search`] query.
    ///
    /// When found, it contains `Some` with the calendar id and name.
    SearchResponse(Option<(String, String)>),
    /// Task counts returned in response to a [`Request::TaskStatus`] query.
    ///
    /// The first field is the number of tasks due today; the second is the number of overdue tasks.
    TaskStatus(u32, u32),
}

/// Options for an import request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportOptions {
    /// Path to the `.ics` file to import.
    pub file: String,
    /// Name of the calendar directory to import into.
    pub calendar: String,
}

async fn acquire_lock(xdg: &BaseDirectories) -> anyhow::Result<File> {
    let path = xdg
        .get_runtime_directory()
        .context("get path to runtime directory")?;
    let path = path.join("eventix.lock");
    task::spawn_blocking(|| {
        let f = File::create(path).context("create eventix.lock")?;
        f.lock_exclusive().context("acquire eventix.lock")?;
        Ok(f)
    })
    .await
    .unwrap()
}

/// Listens for incoming commands on the XDG runtime Unix socket and handles them in a loop.
///
/// Binds a `UnixListener` to the socket path derived from `xdg`, removing any stale socket file
/// first, then processes each connection by reading a [`Request`], dispatching it against `state`,
/// and writing back the [`Response`]. Errors on individual connections are logged but do not
/// terminate the loop. This function runs indefinitely and is intended to be the server-side
/// counterpart of [`send`].
pub async fn handle_commands(xdg: &BaseDirectories, state: EventixState) -> anyhow::Result<()> {
    let socket_path = get_socket_path(xdg);

    // remove it in case it already exists; that's okay because we only get here if the server
    // wasn't running yet.
    std::fs::remove_file(&socket_path).ok();

    let listener = UnixListener::bind(&socket_path)?;
    debug!("cmds: listening on {:?}", socket_path);

    loop {
        match listener.accept().await {
            Ok((mut stream, _addr)) => {
                if let Err(e) = parse_and_handle(state.clone(), &mut stream).await {
                    error!("command failed: {}", e);
                }
            }
            Err(e) => error!("accept failed: {}", e),
        }
    }
}

async fn marshall_msg<T>(stream: &mut UnixStream, data: T) -> anyhow::Result<()>
where
    T: Serialize,
{
    let msg = serde_json::to_string(&data)?;
    stream.write_all(&(msg.len() as u32).to_be_bytes()).await?;
    stream.write_all(msg.as_bytes()).await?;
    Ok(())
}

async fn unmarshall_msg<T>(stream: &mut UnixStream) -> anyhow::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let s = String::from_utf8(buf)?;
    let msg: T = serde_json::from_str(&s)?;
    Ok(msg)
}

async fn parse_and_handle(state: EventixState, stream: &mut UnixStream) -> anyhow::Result<()> {
    let req: Request = unmarshall_msg(stream).await?;
    let resp = handle_request(state, req).await?;
    marshall_msg(stream, resp).await?;
    Ok(())
}

/// Sends a request to the running daemon and returns its response.
///
/// Connects to the Unix socket managed by [`handle_commands`] and returns an error if no daemon
/// is listening.
pub async fn send(xdg: &BaseDirectories, req: Request) -> anyhow::Result<Response> {
    let path = get_socket_path(xdg);
    let stream = UnixStream::connect(&path).await?;
    do_send(xdg, req, stream).await
}

async fn do_send(
    xdg: &BaseDirectories,
    req: Request,
    mut stream: UnixStream,
) -> anyhow::Result<Response> {
    // ensure that not two processes use this socket at the same time
    let _lockfile = acquire_lock(xdg).await?;

    marshall_msg(&mut stream, req).await?;
    let resp: Response = unmarshall_msg(&mut stream).await?;

    Ok(resp)
}

async fn handle_request(state: EventixState, req: Request) -> anyhow::Result<Response> {
    match req {
        Request::Import(req) => handle_import(state, req).await,
        Request::Search(uid) => handle_search(state, uid).await,
        Request::TaskStatus => handle_task_status(state).await,
    }
}

async fn handle_import(state: EventixState, req: ImportOptions) -> anyhow::Result<Response> {
    let save_all = |files: &Vec<CalFile>| {
        for f in files {
            f.save()?;
        }
        Ok(())
    };

    let mut state = state.lock().await;
    let cal = Arc::from(req.calendar.clone());
    let dir = state
        .store_mut()
        .directory_mut(&cal)
        .map_err(anyhow::Error::from)?;

    let files =
        CalFile::new_from_external_file(cal.clone(), dir.path().clone(), req.file.clone().into())
            .context(format!("Parsing file '{}' failed", req.file))?;

    // first delete any existing files with those uids
    for f in &files {
        let uid = f.components().first().unwrap().uid();
        // TODO note that we cannot undo this step
        dir.delete_by_uid(uid).ok();
    }

    // now try to save all and undo these saves, if an error occurs
    if let Err(e) = save_all(&files) {
        for mut f in files {
            f.remove().ok();
        }
        return Err(e);
    }

    // all good; add them to the directory
    for f in files {
        dir.add_file(f).map_err(anyhow::Error::from)?;
    }

    Ok(Response::Success)
}

async fn handle_search(state: EventixState, uid: String) -> anyhow::Result<Response> {
    let state = state.lock().await;
    let res = match state.store().file_by_id(&uid) {
        Some(c_file) => {
            let name = state
                .settings()
                .calendar(c_file.directory())
                .unwrap_or_else(|| panic!("calendar {} not found", c_file.directory()))
                .1
                .name();
            Some(((**c_file.directory()).clone(), name.clone()))
        }
        _ => None,
    };
    Ok(Response::SearchResponse(res))
}

async fn handle_task_status(state: EventixState) -> anyhow::Result<Response> {
    let state = state.lock().await;
    let tz = *state.locale().timezone();
    let today = Local::now().date_naive();

    let due_today = eventix_state::util::due_todos(&state, &tz, 1)
        .filter(|o| o.occurrence_ends_on(today))
        .count();

    let overdue = eventix_state::util::overdue_todos(&state, &tz).count();

    Ok(Response::TaskStatus(due_today as u32, overdue as u32))
}

fn get_socket_path(xdg: &BaseDirectories) -> PathBuf {
    let path = xdg
        .get_runtime_directory()
        .cloned()
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    path.join("eventix.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{path::PathBuf, sync::Arc};

    use eventix_ical::{
        col::{CalDir, CalFile, CalStore},
        objects::{CalComponent, CalEvent, Calendar},
    };
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};
    use tempfile::TempDir;
    use tokio::{net::UnixStream, sync::Mutex};

    // --- helpers ---

    /// Creates an isolated XDG environment inside `dir` and returns `BaseDirectories`
    /// pointing at it.
    fn make_xdg(dir: &TempDir) -> BaseDirectories {
        static XDG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        let root = dir.path();

        // The locale file must be discoverable via XDG_DATA_HOME.
        let locale_dir = root.join("locale");
        std::fs::create_dir_all(&locale_dir).unwrap();
        // Provide an empty-but-valid TOML locale file so `eventix_locale::new` succeeds.
        std::fs::write(locale_dir.join("English.toml"), "[table]\n").unwrap();

        // Create the runtime directory with the required 0700 permissions.
        let runtime = root.join("runtime");
        std::fs::create_dir_all(&runtime).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        }

        // note that guarding the creation of BaseDirectories is sufficient, because it reads all
        // env variables during construction, so that they can be changed again afterwards.
        let _guard = XDG_LOCK.lock().unwrap();
        // SAFETY: the lock above ensures no other thread reads or writes these
        // variables while we hold it, so the unsynchronised write is safe within
        // the test-only context.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", root);
            std::env::set_var("XDG_CONFIG_HOME", root);
            std::env::set_var("XDG_CACHE_HOME", root);
            std::env::set_var("XDG_STATE_HOME", root);
            std::env::set_var("XDG_RUNTIME_DIR", &runtime);
        }
        BaseDirectories::new()
    }

    /// Builds an empty `EventixState` using an isolated XDG temp directory.
    fn make_state(xdg: BaseDirectories) -> EventixState {
        let state = State::new(Arc::new(xdg)).expect("State::new");
        Arc::new(Mutex::new(state))
    }

    /// Builds an `EventixState` that contains an in-memory `CalDir` with the given id,
    /// backed by a real directory on disk so that `CalFile::save` can write to it.
    fn make_state_with_cal(xdg: BaseDirectories, cal_dir: &TempDir, cal_id: &str) -> EventixState {
        let id = Arc::new(cal_id.to_string());
        let dir = CalDir::new_empty(
            id.clone(),
            cal_dir.path().to_path_buf(),
            cal_id.to_string(),
            false,
        );
        let mut store = CalStore::default();
        store.add(dir);

        let mut state = State::new(Arc::new(xdg)).expect("State::new");
        *state.store_mut() = store;
        Arc::new(Mutex::new(state))
    }

    /// Builds an in-memory `CalFile` for the given UID.
    fn make_cal_file(dir_id: &str, dir_path: &std::path::Path, uid: &str) -> CalFile {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Event(CalEvent::new(uid)));
        CalFile::new(
            Arc::new(dir_id.to_string()),
            dir_path.join(format!("{uid}.ics")),
            cal,
        )
    }

    /// Saves a minimal `.ics` file with a single VEVENT to `path`.
    fn write_ics(path: &std::path::Path, uid: &str) {
        let content = format!(
            "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
BEGIN:VEVENT\r\n\
UID:{}\r\n\
SUMMARY:Test\r\n\
DTSTART:20240101T100000Z\r\n\
DTEND:20240101T110000Z\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n",
            uid
        );
        std::fs::write(path, content).unwrap();
    }

    // --- marshall_msg / unmarshall_msg ---

    #[tokio::test]
    async fn marshall_and_unmarshall_roundtrip() {
        let (mut client, mut server) = UnixStream::pair().unwrap();

        let req = Request::TaskStatus;
        marshall_msg(&mut client, &req).await.unwrap();

        let decoded: Request = unmarshall_msg(&mut server).await.unwrap();
        assert!(matches!(decoded, Request::TaskStatus));
    }

    #[tokio::test]
    async fn marshall_and_unmarshall_import_options() {
        let (mut client, mut server) = UnixStream::pair().unwrap();

        let opts = ImportOptions {
            file: "/some/file.ics".into(),
            calendar: "personal".into(),
        };
        let req = Request::Import(opts);
        marshall_msg(&mut client, &req).await.unwrap();

        let decoded: Request = unmarshall_msg(&mut server).await.unwrap();
        match decoded {
            Request::Import(o) => {
                assert_eq!(o.file, "/some/file.ics");
                assert_eq!(o.calendar, "personal");
            }
            _ => panic!("expected Import variant"),
        }
    }

    #[tokio::test]
    async fn marshall_and_unmarshall_response_variants() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        marshall_msg(&mut client, Response::Success).await.unwrap();
        let r: Response = unmarshall_msg(&mut server).await.unwrap();
        assert_eq!(r, Response::Success);

        let (mut client, mut server) = UnixStream::pair().unwrap();
        marshall_msg(&mut client, Response::TaskStatus(2, 5))
            .await
            .unwrap();
        let r: Response = unmarshall_msg(&mut server).await.unwrap();
        assert_eq!(r, Response::TaskStatus(2, 5));
    }

    // --- handle_request / handle_task_status ---

    #[tokio::test]
    async fn handle_task_status_empty_store_returns_zero_counts() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);
        let state = make_state(xdg);

        let resp = handle_request(state, Request::TaskStatus).await.unwrap();
        assert_eq!(resp, Response::TaskStatus(0, 0));
    }

    // --- handle_request / handle_search ---

    #[tokio::test]
    async fn handle_search_find_existing_uid() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);
        let cal_dir = "search";
        let cal_name = "cal-search";
        let col_id = "col-search";
        let cal_tmp = TempDir::new().unwrap();
        let cal_id = Arc::new(cal_dir.to_string());

        // Create the CalFile with the UID we will search for.
        let file = make_cal_file(cal_dir, cal_tmp.path(), "search-uid-1");

        // Build state with the calendar directory and insert the search file.
        let state = make_state_with_cal(xdg, &cal_tmp, cal_dir);

        {
            // Add file to directory
            let mut guard = state.lock().await;
            let dir = guard.store_mut().directory_mut(&cal_id).unwrap();
            dir.add_file(file).unwrap();

            // Add collection and calendar in settings so handle_search can look it up
            let mut col = CollectionSettings::new(SyncerType::FileSystem {
                path: "/data/calendars".to_string(),
            });
            let mut cal_settings = CalendarSettings::default();
            cal_settings.set_enabled(true);
            cal_settings.set_folder("cal".to_string());
            cal_settings.set_name(cal_name.to_string());
            col.all_calendars_mut()
                .insert(cal_id.to_string(), cal_settings);
            guard
                .settings_mut()
                .collections_mut()
                .insert(col_id.to_string(), col);
        }

        let resp = handle_request(state, Request::Search("search-uid-1".to_string()))
            .await
            .unwrap();
        match resp {
            Response::SearchResponse(Some((dname, cname))) => {
                assert_eq!(dname, cal_dir);
                assert_eq!(cname, cal_name);
            }
            _ => panic!("expected SearchResponse(Some(...))"),
        }
    }

    #[tokio::test]
    async fn handle_search_nonexistent_uid_returns_none() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);
        let state = make_state(xdg);

        let resp = handle_request(state, Request::Search("nonexistent-uid".to_string()))
            .await
            .unwrap();
        assert!(matches!(resp, Response::SearchResponse(None)));
    }

    // --- handle_request / handle_import ---

    #[tokio::test]
    async fn handle_import_unknown_calendar_returns_error() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);
        let state = make_state(xdg);

        let opts = ImportOptions {
            file: "/tmp/does-not-matter.ics".into(),
            calendar: "nonexistent".into(),
        };
        let err = handle_request(state, Request::Import(opts))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("nonexistent"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn handle_import_invalid_ics_file_returns_error() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);
        let cal_tmp = TempDir::new().unwrap();
        let state = make_state_with_cal(xdg, &cal_tmp, "test-cal");

        // Create a file that is not valid iCalendar data.
        let bad_ics = tmp.path().join("bad.ics");
        std::fs::write(&bad_ics, "this is not icalendar data").unwrap();

        let opts = ImportOptions {
            file: bad_ics.to_string_lossy().into_owned(),
            calendar: "test-cal".into(),
        };
        let err = handle_request(state, Request::Import(opts))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("bad.ics"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn handle_import_nonexistent_file_returns_error() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);
        let cal_tmp = TempDir::new().unwrap();
        let state = make_state_with_cal(xdg, &cal_tmp, "test-cal");

        let opts = ImportOptions {
            file: "/does/not/exist.ics".into(),
            calendar: "test-cal".into(),
        };
        let err = handle_request(state, Request::Import(opts))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("exist.ics"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn handle_import_valid_ics_adds_file_to_calendar() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);
        let cal_tmp = TempDir::new().unwrap();
        let state = make_state_with_cal(xdg, &cal_tmp, "test-cal");

        let ics_path = tmp.path().join("import.ics");
        write_ics(&ics_path, "import-uid-1");

        let opts = ImportOptions {
            file: ics_path.to_string_lossy().into_owned(),
            calendar: "test-cal".into(),
        };
        let resp = handle_request(state.clone(), Request::Import(opts))
            .await
            .unwrap();
        assert_eq!(resp, Response::Success);

        // The file should now be present in the in-memory calendar directory.
        let locked = state.lock().await;
        let cal_id = Arc::new("test-cal".to_string());
        let dir = locked.store().directory(&cal_id).unwrap();
        assert!(dir.file_by_id("import-uid-1").is_some());
    }

    #[tokio::test]
    async fn handle_import_replaces_existing_uid() {
        let tmp = TempDir::new().unwrap();
        let cal_tmp = TempDir::new().unwrap();
        // Pre-populate the state with a file that has the same UID we will import.
        let cal_id = "test-cal";
        let id = Arc::new(cal_id.to_string());
        let mut dir = CalDir::new_empty(
            id.clone(),
            cal_tmp.path().to_path_buf(),
            cal_id.to_string(),
            false,
        );
        dir.add_file(make_cal_file(cal_id, cal_tmp.path(), "replace-uid"))
            .unwrap();
        let mut store = CalStore::default();
        store.add(dir);

        let mut raw = State::new(Arc::new(make_xdg(&tmp))).expect("State::new");
        *raw.store_mut() = store;
        let state = Arc::new(Mutex::new(raw));

        let ics_path = tmp.path().join("replace.ics");
        write_ics(&ics_path, "replace-uid");

        let opts = ImportOptions {
            file: ics_path.to_string_lossy().into_owned(),
            calendar: "test-cal".into(),
        };
        let resp = handle_request(state.clone(), Request::Import(opts))
            .await
            .unwrap();
        assert_eq!(resp, Response::Success);

        // Exactly one file with the UID should be present after import.
        let locked = state.lock().await;
        let dir = locked.store().directory(&id).unwrap();
        let count = dir.files().iter().filter(|f| {
            f.components()
                .first()
                .map(|c| c.uid().as_str() == "replace-uid")
                .unwrap_or(false)
        });
        assert_eq!(count.count(), 1);
    }

    #[tokio::test]
    async fn handle_import_splits_by_uid_and_keeps_uid_scoped_unknowns() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);
        let cal_tmp = TempDir::new().unwrap();
        let state = make_state_with_cal(xdg, &cal_tmp, "test-cal");

        let ics_path = tmp.path().join("import-split.ics");
        std::fs::write(
            &ics_path,
            "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
BEGIN:VEVENT\r\n\
UID:uid-1\r\n\
DTSTART:20250102T090000Z\r\n\
RECURRENCE-ID:20250102T090000Z\r\n\
DTSTAMP:20250101T000000Z\r\n\
SUMMARY:Override\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:uid-1\r\n\
DTSTART:20250101T090000Z\r\n\
DTSTAMP:20250101T000000Z\r\n\
RRULE:FREQ=DAILY;COUNT=2\r\n\
SUMMARY:Base\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:uid-1\r\n\
DTSTART:20250103T090000Z\r\n\
DTSTAMP:20250101T000000Z\r\n\
SUMMARY:Older duplicate\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:uid-2\r\n\
DTSTART:20250104T090000Z\r\n\
DTEND:20250104T100000Z\r\n\
DTSTAMP:20250101T000000Z\r\n\
SUMMARY:Other\r\n\
END:VEVENT\r\n\
BEGIN:X-GLOBAL\r\n\
X-PROP:global\r\n\
END:X-GLOBAL\r\n\
END:VCALENDAR\r\n",
        )
        .unwrap();

        let opts = ImportOptions {
            file: ics_path.to_string_lossy().into_owned(),
            calendar: "test-cal".into(),
        };
        let resp = handle_request(state.clone(), Request::Import(opts))
            .await
            .unwrap();
        assert_eq!(resp, Response::Success);

        let locked = state.lock().await;
        let cal_id = Arc::new("test-cal".to_string());
        let dir = locked.store().directory(&cal_id).unwrap();
        assert!(dir.file_by_id("uid-1").is_some());
        assert!(dir.file_by_id("uid-2").is_some());
        assert_eq!(dir.files().len(), 2);

        let uid_1 = std::fs::read_to_string(cal_tmp.path().join("uid-1.ics")).unwrap();
        assert!(uid_1.contains("UID:uid-1"));
        assert!(uid_1.contains("SUMMARY:Older duplicate"));
        assert!(uid_1.contains("BEGIN:X-GLOBAL"));
        assert!(!uid_1.contains("UID:uid-2"));

        let uid_2 = std::fs::read_to_string(cal_tmp.path().join("uid-2.ics")).unwrap();
        assert!(uid_2.contains("UID:uid-2"));
        assert!(uid_2.contains("BEGIN:X-GLOBAL"));
        assert!(!uid_2.contains("SUMMARY:Older duplicate"));
        assert!(!uid_2.contains("UID:uid-1"));
    }

    // --- parse_and_handle ---

    #[tokio::test]
    async fn parse_and_handle_task_status_over_stream() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);
        let state = make_state(xdg);

        let (mut client, mut server) = UnixStream::pair().unwrap();

        // Write a TaskStatus request onto the client end.
        marshall_msg(&mut client, Request::TaskStatus)
            .await
            .unwrap();

        // Let the server handle the request and write the response.
        parse_and_handle(state, &mut server).await.unwrap();

        // Read and verify the response from the client end.
        let resp: Response = unmarshall_msg(&mut client).await.unwrap();
        assert_eq!(resp, Response::TaskStatus(0, 0));
    }

    // --- acquire_lock ---

    #[tokio::test]
    async fn acquire_lock_succeeds_with_valid_runtime_dir() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);

        let file = acquire_lock(&xdg).await.unwrap();
        // The lock file should be a valid open file handle.
        drop(file);
    }

    // --- send / handle_commands ---

    /// Spawns a minimal single-request echo server on `socket_path` backed by `state`,
    /// handles exactly one connection, then exits. Returns a `JoinHandle` so the
    /// caller can await completion.
    async fn spawn_one_shot_server(
        socket_path: PathBuf,
        state: EventixState,
    ) -> tokio::task::JoinHandle<()> {
        std::fs::remove_file(&socket_path).ok();
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                parse_and_handle(state, &mut stream).await.ok();
            }
        })
    }

    #[tokio::test]
    async fn send_over_live_socket() {
        let tmp = TempDir::new().unwrap();
        let xdg = make_xdg(&tmp);
        let xdg2 = make_xdg(&tmp);
        let state = make_state(xdg2);

        let socket_path = get_socket_path(&xdg);
        let _server = spawn_one_shot_server(socket_path, state).await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let resp = send(&xdg, Request::TaskStatus).await.unwrap();
        assert_eq!(resp, Response::TaskStatus(0, 0));
    }

    #[tokio::test]
    async fn handle_commands_accepts_and_handles_one_request() {
        // Keep both temp dirs alive for the full duration of the test.
        let tmp = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();

        let xdg = make_xdg(&tmp);
        let xdg2 = make_xdg(&tmp);
        let state = make_state(xdg2);

        let socket_path = get_socket_path(&xdg);

        // Run the command-server loop in a background task; abort it after the test.
        // Wrap xdg in Arc so it can be moved into the spawned future.
        let xdg = Arc::new(xdg);
        let xdg_clone = Arc::clone(&xdg);
        let server_handle = tokio::spawn(async move {
            handle_commands(&xdg_clone, state).await.ok();
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Manually connect to the already-bound socket path and send a request.
        let socket_xdg = Arc::new(make_xdg(&tmp2));
        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let resp = do_send(&socket_xdg, Request::TaskStatus, stream)
            .await
            .unwrap();
        assert_eq!(resp, Response::TaskStatus(0, 0));

        server_handle.abort();
    }
}
