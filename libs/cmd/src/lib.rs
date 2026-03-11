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
//! - [`send_or_execute`] — attempts to reach the daemon first; if the socket is not reachable the
//!   request is executed in-process instead. This is the preferred entry point for CLI commands
//!   that should work regardless of whether the daemon is running.

use anyhow::{Context, anyhow};
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
    /// Query the number of tasks due today and overdue.
    TaskStatus,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Response {
    /// The request completed successfully without a return value.
    Success,
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
/// counterpart of [`send`] and [`send_or_execute`].
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

/// Sends a request to the running daemon if one is reachable, or executes it locally otherwise.
///
/// Attempts to connect to the Unix socket managed by [`handle_commands`]. On success the request
/// is forwarded to the daemon and its response is returned. If the socket is not reachable the
/// request is handled in-process against `state`, so callers do not need to distinguish between
/// the two modes.
pub async fn send_or_execute(
    xdg: &BaseDirectories,
    state: EventixState,
    req: Request,
) -> anyhow::Result<Response> {
    let path = get_socket_path(xdg);
    if let Ok(stream) = UnixStream::connect(&path).await {
        do_send(xdg, req, stream).await
    } else {
        handle_request(state, req).await
    }
}

/// Sends a request to the running daemon and returns its response.
///
/// Connects to the Unix socket managed by [`handle_commands`] and returns an error if no daemon
/// is listening. Prefer [`send_or_execute`] when a local fallback is acceptable.
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
        .ok_or_else(|| anyhow!("Unknown calendar '{}'", req.calendar))?;

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
        dir.add_file(f);
    }

    Ok(Response::Success)
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
    use eventix_state::State;
    use tempfile::TempDir;
    use tokio::{net::UnixStream, sync::Mutex};

    // Serialise every test that mutates XDG environment variables to prevent races when
    // tests run on the default multi-threaded executor. Using `unwrap_or_else` on the lock
    // ensures that a panic in one test (which poisons the mutex) does not cascade to others.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    macro_rules! env_lock {
        () => {
            ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
        };
    }

    // --- helpers ---

    /// Creates an isolated XDG environment inside `dir` and returns `BaseDirectories`
    /// pointing at it. Requires the caller to hold `ENV_LOCK`.
    fn make_xdg(dir: &TempDir) -> BaseDirectories {
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

        // SAFETY: tests that call `make_xdg` must hold `ENV_LOCK` for their entire
        // synchronous setup phase, serialising all env-var mutations so that no two
        // threads observe a partially-written environment at the same time.
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
        let dir = CalDir::new_empty(id.clone(), cal_dir.path().to_path_buf(), cal_id.to_string());
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
        let state = {
            let _guard = env_lock!();
            let tmp = TempDir::new().unwrap();
            let xdg = make_xdg(&tmp);
            make_state(xdg)
        };

        let resp = handle_request(state, Request::TaskStatus).await.unwrap();
        assert_eq!(resp, Response::TaskStatus(0, 0));
    }

    // --- handle_request / handle_import ---

    #[tokio::test]
    async fn handle_import_unknown_calendar_returns_error() {
        let state = {
            let _guard = env_lock!();
            let tmp = TempDir::new().unwrap();
            let xdg = make_xdg(&tmp);
            make_state(xdg)
        };

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
        let (state, bad_ics, _tmp, _cal_tmp) = {
            let _guard = env_lock!();
            let tmp = TempDir::new().unwrap();
            let xdg = make_xdg(&tmp);
            let cal_tmp = TempDir::new().unwrap();
            let state = make_state_with_cal(xdg, &cal_tmp, "test-cal");

            // Create a file that is not valid iCalendar data.
            let bad_ics = tmp.path().join("bad.ics");
            std::fs::write(&bad_ics, "this is not icalendar data").unwrap();
            (state, bad_ics, tmp, cal_tmp)
        };

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
        let state = {
            let _guard = env_lock!();
            let tmp = TempDir::new().unwrap();
            let xdg = make_xdg(&tmp);
            let cal_tmp = TempDir::new().unwrap();
            make_state_with_cal(xdg, &cal_tmp, "test-cal")
        };

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
        let (state, ics_path, _tmp, _cal_tmp) = {
            let _guard = env_lock!();
            let tmp = TempDir::new().unwrap();
            let xdg = make_xdg(&tmp);
            let cal_tmp = TempDir::new().unwrap();
            let state = make_state_with_cal(xdg, &cal_tmp, "test-cal");

            let ics_path = tmp.path().join("import.ics");
            write_ics(&ics_path, "import-uid-1");
            (state, ics_path, tmp, cal_tmp)
        };

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
        let (state, ics_path, id, _tmp, _cal_tmp) = {
            let _guard = env_lock!();
            let tmp = TempDir::new().unwrap();
            let cal_tmp = TempDir::new().unwrap();
            // Pre-populate the state with a file that has the same UID we will import.
            let cal_id = "test-cal";
            let id = Arc::new(cal_id.to_string());
            let mut dir =
                CalDir::new_empty(id.clone(), cal_tmp.path().to_path_buf(), cal_id.to_string());
            dir.add_file(make_cal_file(cal_id, cal_tmp.path(), "replace-uid"));
            let mut store = CalStore::default();
            store.add(dir);

            let mut raw = State::new(Arc::new(make_xdg(&tmp))).expect("State::new");
            *raw.store_mut() = store;
            let state = Arc::new(Mutex::new(raw));

            let ics_path = tmp.path().join("replace.ics");
            write_ics(&ics_path, "replace-uid");
            (state, ics_path, id, tmp, cal_tmp)
        };

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

    // --- send_or_execute: local fallback path ---

    #[tokio::test]
    async fn send_or_execute_falls_back_to_local_when_no_daemon() {
        let (xdg2, state) = {
            let _guard = env_lock!();
            let tmp = TempDir::new().unwrap();
            let xdg = make_xdg(&tmp);
            let state = make_state(xdg);
            // Construct a fresh xdg pointing at a socket path where no daemon is listening.
            let xdg2 = make_xdg(&tmp);
            (xdg2, state)
        };

        let resp = send_or_execute(&xdg2, state, Request::TaskStatus)
            .await
            .unwrap();
        assert_eq!(resp, Response::TaskStatus(0, 0));
    }

    // --- parse_and_handle ---

    #[tokio::test]
    async fn parse_and_handle_task_status_over_stream() {
        let state = {
            let _guard = env_lock!();
            let tmp = TempDir::new().unwrap();
            let xdg = make_xdg(&tmp);
            make_state(xdg)
        };

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
        let (xdg, _tmp) = {
            let _guard = env_lock!();
            let tmp = TempDir::new().unwrap();
            let xdg = make_xdg(&tmp);
            (xdg, tmp)
        };

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
        let (xdg, state, _tmp) = {
            let _guard = env_lock!();
            let tmp = TempDir::new().unwrap();
            let xdg = make_xdg(&tmp);
            let xdg2 = make_xdg(&tmp);
            let state = make_state(xdg2);
            (xdg, state, tmp)
        };

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

        let (xdg, state) = {
            let _guard = env_lock!();
            let xdg = make_xdg(&tmp);
            let xdg2 = make_xdg(&tmp);
            let state = make_state(xdg2);
            (xdg, state)
        };

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
        let socket_xdg = {
            let _guard = env_lock!();
            Arc::new(make_xdg(&tmp2))
        };
        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let resp = do_send(&socket_xdg, Request::TaskStatus, stream)
            .await
            .unwrap();
        assert_eq!(resp, Response::TaskStatus(0, 0));

        server_handle.abort();
    }
}
