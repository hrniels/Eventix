// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, Lines};
use tokio::process::Command;
use tokio::sync::Mutex;
use xdg::BaseDirectories;

use crate::State;
use crate::settings::SyncTimeSpan;
use crate::sync::vdirsyncer::VDirSyncer;
use crate::sync::{SyncColResult, Syncer, SyncerAuth, log_line};

const PORT_BASE: u16 = 25000;

// --- DavmailRunner trait and implementations ---

/// A boxed, heap-allocated future that yields an [`anyhow::Result<SyncColResult>`].
///
/// Not `'static`: may borrow from the calling scope.
type SyncFuture<'a> =
    std::pin::Pin<Box<dyn Future<Output = anyhow::Result<SyncColResult>> + Send + 'a>>;

/// Abstracts the DavMail subprocess lifecycle so that tests can inject a fake runner without
/// spawning a real DavMail process.
pub(crate) trait DavmailRunner: Send + Sync {
    /// Starts DavMail with the given properties file, waits for it to signal readiness, and
    /// then drives `func` to completion.  While `func` is running, monitors DavMail output for
    /// auth requests and handles them using `auth_url` if available.  Kills DavMail when done.
    fn run_with_davmail<'a>(
        &'a self,
        props_path: &'a Path,
        id: &'a str,
        auth_url: Option<&'a String>,
        log: Arc<Mutex<File>>,
        func: SyncFuture<'a>,
    ) -> SyncFuture<'a>;
}

/// Production [`DavmailRunner`] that spawns a real DavMail subprocess.
pub(crate) struct RealDavmailRunner;

impl DavmailRunner for RealDavmailRunner {
    fn run_with_davmail<'a>(
        &'a self,
        props_path: &'a Path,
        id: &'a str,
        auth_url: Option<&'a String>,
        log: Arc<Mutex<File>>,
        func: SyncFuture<'a>,
    ) -> SyncFuture<'a> {
        let props_path = props_path.to_path_buf();
        let id = id.to_string();
        let auth_url = auth_url.cloned();
        Box::pin(async move {
            let mut cmd = Command::new("davmail");
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            cmd.args([props_path.to_str().unwrap()]);
            cmd.kill_on_drop(true);

            let mut child = cmd.spawn()?;
            let stdin = child.stdin.take().unwrap();
            let stdout = child.stdout.take().unwrap();
            let mut reader = BufReader::new(stdout).lines();

            run_with_davmail_impl(
                stdin,
                &mut reader,
                || async move {
                    child.kill().await.ok();
                },
                &id,
                auth_url.as_ref(),
                log,
                func,
            )
            .await
        })
    }
}

/// Core DavMail lifecycle logic: waits for the readiness line, then races the caller-supplied
/// sync future against stdout monitoring for auth requests.
///
/// Accepts pre-constructed I/O handles so that tests can inject in-memory pipes without spawning
/// a real DavMail process. `kill` is called unconditionally when the function returns.
async fn run_with_davmail_impl<W, R, K, Fut>(
    mut stdin: W,
    reader: &mut Lines<BufReader<R>>,
    kill: K,
    id: &str,
    auth_url: Option<&String>,
    log: Arc<Mutex<File>>,
    func: SyncFuture<'_>,
) -> anyhow::Result<SyncColResult>
where
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
    K: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    // Wait until DavMail signals that it is ready to accept connections.
    while let Ok(Some(line)) = reader.next_line().await {
        log_line(&log, id, &line).await?;
        if line.contains("Start DavMail in server mode") {
            break;
        }
    }

    // Read lines from DavMail stdout and watch for auth requests.
    let mut read_output = async || {
        while let Ok(Some(line)) = reader.next_line().await {
            log_line(&log, id, &line).await?;

            // do we need to (re-)authenticate?
            if line.starts_with("https://login.microsoftonline.com/") {
                // if we already have the URL from the user, tell DavMail about it
                if let Some(auth_url) = auth_url {
                    stdin.write_all(auth_url.as_bytes()).await?;
                    stdin.write_all(b"\n").await?;
                } else {
                    // otherwise we fail and ask the user to authenticate
                    return Ok(SyncColResult::AuthFailed(line));
                }
            }
        }
        Err(anyhow!("DavMail exited first"))
    };

    // Race the caller-supplied sync function against DavMail's stdout monitor. Whichever
    // branch completes first wins: if the sync finishes, we get its result; if DavMail exits
    // (or signals an auth requirement) before the sync completes, we get that error instead.
    let res = tokio::select! {
        res = func => res,
        res = read_output() => res,
    };
    kill().await;
    res
}

// --- O365 ---

/// A [`Syncer`] implementation that synchronises Microsoft 365 calendars via DavMail and
/// vdirsyncer.
///
/// Manages a generated DavMail properties file and spawns a DavMail subprocess as a local CalDAV
/// gateway, then delegates all sync operations to an inner [`VDirSyncer`]. OAuth refresh tokens
/// are persisted to the misc state after each successful operation.
pub struct O365 {
    col_id: String,
    vdirsyncer: VDirSyncer,
    auth_url: Option<String>,
    props_path: PathBuf,
    pending_token: Option<String>,
    log: Arc<Mutex<File>>,
    runner: Arc<dyn DavmailRunner>,
}

impl O365 {
    /// Creates a new `O365` syncer, generating the DavMail properties file and inner
    /// `VDirSyncer` on disk.
    ///
    /// `idx` is used to derive a unique local port for the DavMail CalDAV gateway. `auth_url` is
    /// the Microsoft login redirect URL provided by the user during an interactive auth flow;
    /// `token` is a previously persisted OAuth refresh token that can skip re-authentication.
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        xdg: &BaseDirectories,
        idx: usize,
        col_id: String,
        folder_id: HashMap<String, String>,
        read_only: bool,
        auth: SyncerAuth,
        auth_url: Option<&String>,
        token: Option<String>,
        time_span: &SyncTimeSpan,
        log: Arc<Mutex<File>>,
    ) -> anyhow::Result<Self> {
        let port = PORT_BASE + idx as u16;

        // generate properties file
        let props_path = Self::generate_props(xdg, &col_id, port, &auth.user, token).await?;

        // build URL
        let user_enc = utf8_percent_encode(&auth.user, NON_ALPHANUMERIC).to_string();
        let col_enc = utf8_percent_encode(&col_id, NON_ALPHANUMERIC).to_string();
        let url = format!("http://localhost:{}/users/{}/{}/", port, user_enc, col_enc);

        // create vdirsyncer instance
        let vdirsyncer = VDirSyncer::new(
            xdg,
            col_id.clone(),
            folder_id,
            url,
            read_only,
            Some(auth),
            time_span,
            log.clone(),
        )
        .await?;

        Ok(Self::new_with_runner(
            col_id,
            vdirsyncer,
            auth_url,
            props_path,
            log,
            Arc::new(RealDavmailRunner),
        ))
    }

    /// Creates a new `O365` from an already-constructed [`VDirSyncer`] and properties file path,
    /// with a custom [`DavmailRunner`], for use in tests.
    pub(crate) fn new_with_runner(
        col_id: String,
        vdirsyncer: VDirSyncer,
        auth_url: Option<&String>,
        props_path: PathBuf,
        log: Arc<Mutex<File>>,
        runner: Arc<dyn DavmailRunner>,
    ) -> Self {
        Self {
            col_id,
            vdirsyncer,
            auth_url: auth_url.cloned(),
            props_path,
            pending_token: None,
            log,
            runner,
        }
    }

    /// Generates a DavMail `.properties` file for the collection identified by `name`.
    ///
    /// Writes all required DavMail configuration keys, including the CalDAV port, the
    /// bound address, and optional OAuth refresh token. Returns the path to the written
    /// file, or an error if the file cannot be created.
    pub(crate) async fn generate_props(
        xdg: &BaseDirectories,
        name: &String,
        port: u16,
        user: &String,
        token: Option<String>,
    ) -> anyhow::Result<PathBuf> {
        let dir = xdg.get_data_file("vdirsyncer").unwrap();

        let props_path = dir.join(format!("{}-davmail.properties", name));
        let mut props = File::options()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&props_path)
            .await?;

        props.write_all(b"davmail.server=true\n").await?;
        props.write_all(b"davmail.mode=O365Manual\n").await?;
        props.write_all(b"davmail.enableOidc=true\n").await?;
        props
            .write_all(b"davmail.oauth.persistToken=true\n")
            .await?;
        props.write_all(b"davmail.bindAddress=127.0.0.1\n").await?;
        props
            .write_all(format!("davmail.caldavPort={}\n", port).as_bytes())
            .await?;
        props.write_all(b"davmail.allowRemote=false\n").await?;
        props
            .write_all(b"davmail.disableUpdateCheck=true\n")
            .await?;
        props.write_all(b"davmail.enableKeepAlive=true\n").await?;
        props.write_all(b"davmail.folderSizeLimit=0\n").await?;
        props.write_all(b"davmail.defaultDomain=\n").await?;
        props.write_all(b"davmail.logFilePath=/dev/null\n").await?;
        props.write_all(b"log4j.logger.davmail=DEBUG\n").await?;
        props
            .write_all(b"davmail.disableGuiNotifications=true\n")
            .await?;
        props
            .write_all(b"davmail.disableTrayActivitySwitch=true\n")
            .await?;
        props
            .write_all(b"davmail.showStartupBanner=false\n")
            .await?;
        if let Some(token) = token {
            props
                .write_all(format!("davmail.oauth.{}.refreshToken={}\n", user, token).as_bytes())
                .await?;
        }

        // tokio::fs::File does not flush on drop; explicitly flush so all buffered writes reach
        // the filesystem before the caller reads the file back.
        props.flush().await?;

        Ok(props_path)
    }

    async fn remember_token(
        &mut self,
        res: anyhow::Result<SyncColResult>,
    ) -> anyhow::Result<SyncColResult> {
        // Only persist the token when the sync succeeded AND this was an interactive auth flow
        // (auth_url is Some). Background syncs that reuse a stored token should not overwrite
        // the persisted token, because DavMail may not have written a fresh one.
        if let Ok(SyncColResult::Success(_)) = res
            && self.auth_url.is_some()
        {
            let file = File::options().read(true).open(&self.props_path).await?;
            let reader = BufReader::new(file);
            let mut lines = reader.lines();
            while let Some(line) = lines.next_line().await? {
                // extract the token from the changed properties file
                if line.contains("refreshToken=")
                    && let Some(split) = line.find('=')
                {
                    let token = &line[split + 1..];
                    self.pending_token = Some(token.to_string());
                    break;
                }
            }
        }
        res
    }

    fn take_token(&mut self) -> Option<String> {
        self.pending_token.take()
    }
}

#[async_trait]
impl Syncer for O365 {
    async fn discover(&mut self) -> anyhow::Result<SyncColResult> {
        let id = self.col_id.clone();
        let auth_url = self.auth_url.clone();
        let props_path = self.props_path.clone();
        let log = self.log.clone();
        // Clone the runner Arc so the borrow of `self` ends before we need `self` in the future.
        let runner = self.runner.clone();

        let res = runner
            .run_with_davmail(
                &props_path,
                &id,
                auth_url.as_ref(),
                log,
                Box::pin(async { self.vdirsyncer.discover().await }),
            )
            .await;

        self.remember_token(res).await
    }

    async fn sync_cal(&mut self, cal_id: &String) -> anyhow::Result<SyncColResult> {
        let id = self.col_id.clone();
        let auth_url = self.auth_url.clone();
        let props_path = self.props_path.clone();
        let log = self.log.clone();
        let runner = self.runner.clone();

        let res = runner
            .run_with_davmail(
                &props_path,
                &id,
                auth_url.as_ref(),
                log,
                Box::pin(async { self.vdirsyncer.sync_cal(cal_id).await }),
            )
            .await;

        self.remember_token(res).await
    }

    async fn sync(&mut self) -> anyhow::Result<SyncColResult> {
        let id = self.col_id.clone();
        let auth_url = self.auth_url.clone();
        let props_path = self.props_path.clone();
        let log = self.log.clone();
        let runner = self.runner.clone();

        let res = runner
            .run_with_davmail(
                &props_path,
                &id,
                auth_url.as_ref(),
                log,
                Box::pin(async { self.vdirsyncer.sync().await }),
            )
            .await;

        self.remember_token(res).await
    }

    async fn delete_cal(&mut self, cal_id: &String) -> anyhow::Result<()> {
        self.vdirsyncer.delete_cal(cal_id).await
    }

    async fn create_cal_by_folder(&mut self, folder: &String) -> anyhow::Result<()> {
        self.vdirsyncer.create_cal_by_folder(folder).await
    }

    async fn delete_cal_by_folder(&mut self, folder: &String) -> anyhow::Result<()> {
        self.vdirsyncer.delete_cal_by_folder(folder).await
    }

    async fn delete(&mut self, all: bool) -> anyhow::Result<()> {
        self.vdirsyncer.delete(all).await?;

        if all {
            // remove generated property file
            fs::remove_file(&self.props_path).await.context(format!(
                "Deleting {} failed",
                self.props_path.to_str().unwrap()
            ))
        } else {
            Ok(())
        }
    }

    fn finish(&mut self, state: &mut State, _result: &mut SyncColResult) -> anyhow::Result<()> {
        if let Some(token) = self.take_token() {
            let misc = state.misc_mut();
            misc.set_collection_token(&self.col_id, token);
            misc.write_to_file()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::process::{ExitStatus, Output};
    use std::sync::Mutex as StdMutex;
    use std::task::{Context as TaskContext, Poll};
    use tokio::io::AsyncReadExt;
    use tokio::sync::Notify;

    // --- FakeCommandRunner (for injecting into inner VDirSyncer) ---

    /// A recorded call made to the fake runner.
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    enum RunCall {
        Run { program: String, args: Vec<String> },
        RunInteractive { program: String, args: Vec<String> },
    }

    /// Canned response for one `run` call on the inner `VDirSyncer`.
    struct CannedOutput {
        stderr: Vec<u8>,
        /// `None` means success (exit code 0).
        exit_code: Option<i32>,
    }

    impl CannedOutput {
        fn success(stderr: impl Into<Vec<u8>>) -> Self {
            Self {
                stderr: stderr.into(),
                exit_code: None,
            }
        }
    }

    struct FakeCommandRunner {
        run_responses: StdMutex<Vec<CannedOutput>>,
        interactive_responses: StdMutex<Vec<(ExitStatus, Vec<String>)>>,
        calls: StdMutex<Vec<RunCall>>,
    }

    impl FakeCommandRunner {
        fn new(
            run_responses: Vec<CannedOutput>,
            interactive_responses: Vec<(ExitStatus, Vec<String>)>,
        ) -> Arc<Self> {
            Arc::new(Self {
                run_responses: StdMutex::new(run_responses),
                interactive_responses: StdMutex::new(interactive_responses),
                calls: StdMutex::new(vec![]),
            })
        }

        fn empty() -> Arc<Self> {
            Self::new(vec![], vec![])
        }

        fn calls(&self) -> Vec<RunCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl crate::sync::vdirsyncer::CommandRunner for FakeCommandRunner {
        async fn run(
            &self,
            program: &str,
            args: &[&str],
            _stdin_data: Option<&[u8]>,
        ) -> anyhow::Result<Output> {
            self.calls.lock().unwrap().push(RunCall::Run {
                program: program.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
            });
            let canned = self.run_responses.lock().unwrap().remove(0);
            let status = match canned.exit_code {
                None => ExitStatus::from_raw(0),
                Some(code) => ExitStatus::from_raw(code << 8),
            };
            Ok(Output {
                status,
                stdout: vec![],
                stderr: canned.stderr,
            })
        }

        async fn run_interactive(
            &self,
            program: &str,
            args: &[&str],
            _yes_response: &[u8],
        ) -> anyhow::Result<(ExitStatus, Vec<String>)> {
            self.calls.lock().unwrap().push(RunCall::RunInteractive {
                program: program.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
            });
            Ok(self.interactive_responses.lock().unwrap().remove(0))
        }
    }

    // --- FakeDavmailRunner ---

    /// Controls what the fake DavMail runner does when invoked.
    enum FakeDavmailBehavior {
        /// Just call through to `func()` directly (DavMail is always ready, no auth needed).
        CallThrough,
        /// Return `AuthFailed` immediately without calling `func`.
        AuthFailed(String),
    }

    struct FakeDavmailRunner {
        behavior: FakeDavmailBehavior,
    }

    impl FakeDavmailRunner {
        fn call_through() -> Arc<Self> {
            Arc::new(Self {
                behavior: FakeDavmailBehavior::CallThrough,
            })
        }

        fn auth_failed(url: &str) -> Arc<Self> {
            Arc::new(Self {
                behavior: FakeDavmailBehavior::AuthFailed(url.to_string()),
            })
        }
    }

    impl DavmailRunner for FakeDavmailRunner {
        fn run_with_davmail<'a>(
            &'a self,
            _props_path: &'a Path,
            _id: &'a str,
            _auth_url: Option<&'a String>,
            _log: Arc<Mutex<File>>,
            func: SyncFuture<'a>,
        ) -> SyncFuture<'a> {
            match &self.behavior {
                FakeDavmailBehavior::CallThrough => func,
                FakeDavmailBehavior::AuthFailed(url) => {
                    let url = url.clone();
                    Box::pin(async move { Ok(SyncColResult::AuthFailed(url)) })
                }
            }
        }
    }

    // --- Helpers ---

    async fn make_log(dir: &std::path::Path) -> Arc<Mutex<File>> {
        let log_path = dir.join("test.log");
        let f = tokio::fs::File::options()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
            .unwrap();
        Arc::new(Mutex::new(f))
    }

    /// Creates a `BaseDirectories` rooted in `tmp` by setting XDG env vars.
    ///
    /// Returns the configured `BaseDirectories` and also creates the `vdirsyncer` data
    /// subdirectory so that `generate_props` and `VDirSyncer::new` can write files immediately.
    async fn make_xdg(tmp: &tempfile::TempDir) -> BaseDirectories {
        let xdg = crate::with_test_xdg(&tmp.path().join("data"), &tmp.path().join("config"));
        let vdir: PathBuf = xdg.get_data_file("vdirsyncer").unwrap();
        tokio::fs::create_dir_all(&vdir).await.unwrap();
        xdg
    }

    fn make_auth() -> SyncerAuth {
        SyncerAuth {
            user: "user@example.com".to_string(),
            pw_cmd: vec!["echo".to_string(), "secret".to_string()],
        }
    }

    /// Builds a minimal `O365` using the supplied fake runners.
    ///
    /// The inner `VDirSyncer` is configured with `col_id = "mycol"` and `folder_id`.
    /// Returns the `O365` instance; the `TempDir` must be kept alive by the caller.
    async fn make_o365_with_runners(
        tmp: &tempfile::TempDir,
        folder_id: HashMap<String, String>,
        vdir_runner: Arc<dyn crate::sync::vdirsyncer::CommandRunner>,
        davmail_runner: Arc<dyn DavmailRunner>,
        auth_url: Option<&String>,
    ) -> O365 {
        let xdg = make_xdg(tmp).await;
        let log = make_log(tmp.path()).await;
        let auth = make_auth();
        let col_id = "mycol".to_string();

        let vdirsyncer = VDirSyncer::new_with_runner(
            &xdg,
            col_id.clone(),
            folder_id,
            "http://localhost:25000/users/user/mycol/".to_string(),
            false,
            Some(auth),
            &crate::settings::SyncTimeSpan::default(),
            log.clone(),
            vdir_runner,
        )
        .await
        .unwrap();

        let props_path =
            O365::generate_props(&xdg, &col_id, 25000, &"user@example.com".to_string(), None)
                .await
                .unwrap();

        O365::new_with_runner(
            col_id,
            vdirsyncer,
            auth_url,
            props_path,
            log,
            davmail_runner,
        )
    }

    /// Test helper: create a tempdir, a FakeCommandRunner pre-seeded with canned responses,
    /// and an O365 wired to those runners. Returns (tmpdir, fake_runner, o365).
    async fn setup_o365_with_vdir(
        run_responses: Vec<CannedOutput>,
        interactive_responses: Vec<(ExitStatus, Vec<String>)>,
        folder_id: HashMap<String, String>,
        davmail_runner: Arc<dyn DavmailRunner>,
        auth_url: Option<&String>,
    ) -> (tempfile::TempDir, Arc<FakeCommandRunner>, O365) {
        let tmp = tempfile::tempdir().unwrap();
        let vdir_runner = FakeCommandRunner::new(run_responses, interactive_responses);
        let vdir_runner_ref = vdir_runner.clone();

        let o365 =
            make_o365_with_runners(&tmp, folder_id, vdir_runner_ref, davmail_runner, auth_url)
                .await;
        (tmp, vdir_runner, o365)
    }

    // --- generate_props tests ---

    #[tokio::test]
    async fn generate_props_writes_required_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = make_xdg(&tmp).await;

        let name = "mycol".to_string();
        let user = "user@example.com".to_string();
        let port = 25001u16;

        let path = O365::generate_props(&xdg, &name, port, &user, None)
            .await
            .unwrap();

        let mut content = String::new();
        tokio::fs::File::open(&path)
            .await
            .unwrap()
            .read_to_string(&mut content)
            .await
            .unwrap();

        assert!(content.contains("davmail.server=true"));
        assert!(content.contains("davmail.mode=O365Manual"));
        assert!(content.contains(&format!("davmail.caldavPort={}", port)));
        assert!(content.contains("davmail.bindAddress=127.0.0.1"));
        assert!(content.contains("davmail.allowRemote=false"));
        assert!(
            !content.contains("refreshToken"),
            "no token when token is None"
        );
    }

    #[tokio::test]
    async fn generate_props_writes_token_when_provided() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = make_xdg(&tmp).await;

        let name = "mycol".to_string();
        let user = "user@example.com".to_string();

        let path = O365::generate_props(
            &xdg,
            &name,
            25001,
            &user,
            Some("my-refresh-token".to_string()),
        )
        .await
        .unwrap();

        let mut content = String::new();
        tokio::fs::File::open(&path)
            .await
            .unwrap()
            .read_to_string(&mut content)
            .await
            .unwrap();

        assert!(content.contains("refreshToken=my-refresh-token"));
    }

    // --- FakeDavmailRunner behaviour tests ---

    #[tokio::test]
    async fn fake_runner_auth_failed_does_not_call_func() {
        let url = "https://login.microsoftonline.com/auth";
        let runner = FakeDavmailRunner::auth_failed(url);
        let tmp = tempfile::tempdir().unwrap();
        let log = make_log(tmp.path()).await;

        let func_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let func_called_clone = func_called.clone();
        let result = runner
            .run_with_davmail(
                Path::new("/irrelevant"),
                "test",
                None,
                log,
                Box::pin(async move {
                    func_called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(SyncColResult::Success(false))
                }),
            )
            .await
            .unwrap();

        assert!(!func_called.load(std::sync::atomic::Ordering::SeqCst));
        let SyncColResult::AuthFailed(returned_url) = result else {
            panic!("expected AuthFailed");
        };
        assert_eq!(returned_url, url);
    }

    // --- O365 Syncer delegation tests ---
    //
    // These tests use FakeDavmailRunner::call_through with a non-empty collection so that the
    // inner VDirSyncer actually invokes CommandRunner::run, proving the full chain:
    //   O365::sync → DavmailRunner::run_with_davmail → VDirSyncer::sync → CommandRunner::run

    #[tokio::test]
    async fn sync_invokes_vdirsyncer_runner() {
        let mut folder_id = HashMap::new();
        folder_id.insert("work".to_string(), "cal-1".to_string());

        let (_tmp, vdir_runner, mut o365) = setup_o365_with_vdir(
            vec![CannedOutput::success(b"")],
            vec![],
            folder_id,
            FakeDavmailRunner::call_through(),
            None,
        )
        .await;

        let res = o365.sync().await.unwrap();
        assert_eq!(res, SyncColResult::Success(false));

        let calls = vdir_runner.calls();
        assert_eq!(calls.len(), 1);
        let RunCall::Run { args, .. } = &calls[0] else {
            panic!("expected Run call, got {:?}", calls[0]);
        };
        assert!(
            args.contains(&"sync".to_string()),
            "args should contain 'sync': {:?}",
            args
        );
        assert!(
            args.iter().any(|a| a.contains("mycol/work")),
            "args should contain pair name: {:?}",
            args
        );
    }

    #[tokio::test]
    async fn sync_cal_invokes_vdirsyncer_runner() {
        let mut folder_id = HashMap::new();
        folder_id.insert("work".to_string(), "cal-1".to_string());

        let (_tmp, vdir_runner, mut o365) = setup_o365_with_vdir(
            vec![CannedOutput::success(b"")],
            vec![],
            folder_id,
            FakeDavmailRunner::call_through(),
            None,
        )
        .await;

        let res = o365.sync_cal(&"cal-1".to_string()).await.unwrap();
        assert_eq!(res, SyncColResult::Success(false));

        let calls = vdir_runner.calls();
        assert_eq!(calls.len(), 1);
        let RunCall::Run { args, .. } = &calls[0] else {
            panic!("expected Run call");
        };
        assert!(args.contains(&"sync".to_string()));
        assert!(args.iter().any(|a| a.contains("mycol/work")));
    }

    #[tokio::test]
    async fn discover_invokes_vdirsyncer_runner() {
        // discover calls run_interactive (discover) then run (metasync).
        let ok_status = ExitStatus::from_raw(0);
        let (_tmp, vdir_runner, mut o365) = setup_o365_with_vdir(
            vec![CannedOutput::success(b"")],
            vec![(ok_status, vec![])],
            HashMap::new(),
            FakeDavmailRunner::call_through(),
            None,
        )
        .await;

        let res = o365.discover().await.unwrap();
        assert_eq!(res, SyncColResult::Success(true));

        let calls = vdir_runner.calls();
        assert_eq!(calls.len(), 2);
        assert!(matches!(&calls[0], RunCall::RunInteractive { args, .. }
            if args.contains(&"discover".to_string())));
        assert!(matches!(&calls[1], RunCall::Run { args, .. }
            if args.contains(&"metasync".to_string())));
    }

    #[tokio::test]
    async fn auth_failed_when_davmail_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let url = "https://login.microsoftonline.com/auth";
        let mut o365 = make_o365_with_runners(
            &tmp,
            HashMap::new(),
            FakeCommandRunner::empty(),
            FakeDavmailRunner::auth_failed(url),
            None,
        )
        .await;

        let res = o365.sync().await.unwrap();
        assert_eq!(res, SyncColResult::AuthFailed(url.to_string()));

        let res = o365.sync_cal(&"cal-1".to_string()).await.unwrap();
        assert_eq!(res, SyncColResult::AuthFailed(url.to_string()));

        let res = o365.discover().await.unwrap();
        assert_eq!(res, SyncColResult::AuthFailed(url.to_string()));
    }

    // --- O365::delete / delete_cal tests ---

    #[tokio::test]
    async fn delete_with_config_removes_props_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut o365 = make_o365_with_runners(
            &tmp,
            HashMap::new(),
            FakeCommandRunner::empty(),
            FakeDavmailRunner::call_through(),
            None,
        )
        .await;

        let props_path = o365.props_path.clone();
        assert!(props_path.exists(), "props file should exist before delete");

        o365.delete(false).await.unwrap();
        assert!(
            props_path.exists(),
            "props file should be kept when config=false"
        );

        o365.delete(true).await.unwrap();
        assert!(
            !props_path.exists(),
            "props file should be removed when config=true"
        );
    }

    // --- remember_token test ---

    #[tokio::test]
    async fn remember_token_persists_token_on_success() {
        let tmp = tempfile::tempdir().unwrap();
        let auth_url = "https://login.microsoftonline.com/redirect?code=abc".to_string();
        let mut o365 = make_o365_with_runners(
            &tmp,
            HashMap::new(),
            FakeCommandRunner::empty(),
            FakeDavmailRunner::call_through(),
            Some(&auth_url),
        )
        .await;

        // Simulate DavMail writing a refreshToken line to the props file.
        tokio::fs::write(
            &o365.props_path,
            "davmail.oauth.user@example.com.refreshToken=super-secret-token\n",
        )
        .await
        .unwrap();

        let res = o365.sync().await.unwrap();
        assert_eq!(res, SyncColResult::Success(false));

        assert_eq!(o365.take_token(), Some("super-secret-token".to_string()),);
    }

    // --- run_with_davmail_impl unit tests ---
    //
    // These tests exercise the core DavMail lifecycle logic (readiness detection, auth URL
    // injection, select! racing) without spawning a real DavMail process.  In-memory duplex
    // channels simulate DavMail's stdin/stdout.
    //
    // Design note: tokio::select! polls both branches on the first call.  To make tests
    // deterministic we keep the stdout *writer* alive (so read_output's next_line() blocks
    // rather than returning EOF immediately).  Where a test needs to verify that read_output
    // processed a specific line *before* func wins the select!, we use NotifyOnWrite: a wrapper
    // around the stdin writer that fires a Notify the moment any bytes are written to it.  func
    // then awaits that notification, making the ordering deterministic without relying on
    // yield_now() counts that are sensitive to concurrent test execution.

    /// Wraps an `AsyncWrite` and fires a `Notify` the first time any bytes are written through it.
    struct NotifyOnWrite<W> {
        inner: W,
        notify: Arc<Notify>,
        notified: bool,
    }

    impl<W> NotifyOnWrite<W> {
        fn new(inner: W, notify: Arc<Notify>) -> Self {
            Self {
                inner,
                notify,
                notified: false,
            }
        }
    }

    impl<W: AsyncWrite + Unpin> AsyncWrite for NotifyOnWrite<W> {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            let res = Pin::new(&mut self.inner).poll_write(cx, buf);
            if matches!(res, Poll::Ready(Ok(_))) && !self.notified {
                self.notified = true;
                self.notify.notify_one();
            }
            res
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    #[tokio::test]
    async fn impl_calls_func_after_ready_line() {
        // Spawn a task that writes the readiness line and then keeps the writer open.
        // read_output will block on the next next_line() call, giving func time to win.
        let (mut writer, reader) = tokio::io::duplex(4096);
        writer
            .write_all(b"Start DavMail in server mode\n")
            .await
            .unwrap();
        // writer stays alive (not dropped) so read_output blocks after consuming the line.
        let mut reader_lines = BufReader::new(reader).lines();

        let tmp = tempfile::tempdir().unwrap();
        let log = make_log(tmp.path()).await;
        let stdin = tokio::io::sink();
        let killed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let killed_clone = killed.clone();

        let res = run_with_davmail_impl(
            stdin,
            &mut reader_lines,
            || async move {
                killed_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            },
            "test",
            None,
            log,
            // Yield once so read_output can reach its blocking next_line() call, then return.
            Box::pin(async {
                tokio::task::yield_now().await;
                Ok(SyncColResult::Success(true))
            }),
        )
        .await
        .unwrap();

        assert_eq!(res, SyncColResult::Success(true));
        assert!(
            killed.load(std::sync::atomic::Ordering::SeqCst),
            "kill should be called"
        );
        drop(writer); // keep alive until here
    }

    #[tokio::test]
    async fn impl_returns_auth_failed_when_no_auth_url() {
        // stdout: readiness line, then a Microsoft login URL.  No auth_url provided → AuthFailed.
        // func blocks forever; the stdout branch wins via the auth line.
        let (mut writer, reader) = tokio::io::duplex(4096);
        writer
            .write_all(
                b"Start DavMail in server mode\nhttps://login.microsoftonline.com/tenant/oauth2\n",
            )
            .await
            .unwrap();
        // Keep writer open so there is no spurious EOF after the auth line.
        let mut reader_lines = BufReader::new(reader).lines();

        let tmp = tempfile::tempdir().unwrap();
        let log = make_log(tmp.path()).await;
        let stdin = tokio::io::sink();

        let res = run_with_davmail_impl(
            stdin,
            &mut reader_lines,
            || async {},
            "test",
            None,
            log,
            Box::pin(std::future::pending()),
        )
        .await
        .unwrap();

        drop(writer);
        let SyncColResult::AuthFailed(url) = res else {
            panic!("expected AuthFailed, got {:?}", res);
        };
        assert!(url.starts_with("https://login.microsoftonline.com/"));
    }

    #[tokio::test]
    async fn impl_feeds_auth_url_to_stdin_and_func_succeeds() {
        // stdout: readiness line, then auth URL line.  auth_url is provided → stdin receives it.
        // func waits until the auth URL has been written to stdin (via NotifyOnWrite), then wins.
        let (mut writer, reader) = tokio::io::duplex(4096);
        writer
            .write_all(
                b"Start DavMail in server mode\nhttps://login.microsoftonline.com/tenant/oauth2\n",
            )
            .await
            .unwrap();
        // Keep writer open so read_output blocks after writing auth URL to stdin.
        let mut reader_lines = BufReader::new(reader).lines();

        let tmp = tempfile::tempdir().unwrap();
        let log = make_log(tmp.path()).await;

        // Use a duplex channel for stdin so we can read back what was written.
        let (stdin_write, mut stdin_read) = tokio::io::duplex(256);

        // Wrap stdin_write so that func can wait until read_output has actually written to it.
        let written_notify = Arc::new(Notify::new());
        let stdin_notifying = NotifyOnWrite::new(stdin_write, written_notify.clone());

        let auth_url = "https://login.microsoftonline.com/redirect?code=xyz".to_string();

        let res = run_with_davmail_impl(
            stdin_notifying,
            &mut reader_lines,
            || async {},
            "test",
            Some(&auth_url),
            log,
            // Wait until read_output has written the auth URL to stdin before returning.
            Box::pin(async move {
                written_notify.notified().await;
                Ok(SyncColResult::Success(true))
            }),
        )
        .await
        .unwrap();

        drop(writer);
        assert_eq!(res, SyncColResult::Success(true));

        // Verify that the auth URL was written to stdin (stdin_write was dropped by impl).
        let mut written = Vec::new();
        stdin_read.read_to_end(&mut written).await.unwrap();
        assert!(
            written.starts_with(auth_url.as_bytes()),
            "stdin should start with auth_url, got: {:?}",
            String::from_utf8_lossy(&written)
        );
    }

    #[tokio::test]
    async fn impl_returns_error_when_stdout_closes_before_ready() {
        // stdout closes immediately without emitting the readiness line: DavMail exited early.
        // The readiness loop ends; then read_output() returns Err("DavMail exited first")
        // immediately (before func produces a result).
        let (writer, reader) = tokio::io::duplex(4096);
        drop(writer); // EOF immediately
        let mut reader_lines = BufReader::new(reader).lines();

        let tmp = tempfile::tempdir().unwrap();
        let log = make_log(tmp.path()).await;
        let stdin = tokio::io::sink();

        let res = run_with_davmail_impl(
            stdin,
            &mut reader_lines,
            || async {},
            "test",
            None,
            log,
            Box::pin(std::future::pending()),
        )
        .await;

        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("DavMail exited first"),
            "error message should mention DavMail exited"
        );
    }
}
