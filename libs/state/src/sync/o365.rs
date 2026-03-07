use anyhow::{Context, anyhow};
use async_trait::async_trait;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use xdg::BaseDirectories;

use crate::State;
use crate::sync::vdirsyncer::VDirSyncer;
use crate::sync::{SyncColResult, Syncer, SyncerAuth, log_line};

const PORT_BASE: u16 = 25000;

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
    log: Arc<Mutex<File>>,
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
            log.clone(),
        )
        .await?;

        Ok(Self {
            col_id,
            vdirsyncer,
            auth_url: auth_url.cloned(),
            props_path,
            log,
        })
    }

    async fn generate_props(
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
        Ok(props_path)
    }

    async fn remember_token(
        &self,
        state: &mut State,
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
                    // permanently remember the token
                    let misc = state.misc_mut();
                    misc.set_collection_token(&self.col_id, token.to_string());
                    misc.write_to_file()?;
                    break;
                }
            }
        }
        res
    }

    async fn with_davmail<F, Fut>(
        props_path: &Path,
        id: &str,
        auth_url: Option<&String>,
        log: Arc<Mutex<File>>,
        func: F,
    ) -> anyhow::Result<SyncColResult>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<SyncColResult>>,
    {
        let mut cmd = Command::new("davmail");
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.args([props_path.to_str().unwrap()]);
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout).lines();

        // ensure that the server is started before we let vdirsyncer connect to it
        while let Ok(Some(line)) = reader.next_line().await {
            log_line(&log, id, &line).await?;
            if line.contains("Start DavMail in server mode") {
                break;
            }
        }

        // read lines and watch for auth requests
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
            // wait until the function finished
            res = func() => {
                res
            }
            // in the meantime, read all printed lines from davmail
            res = read_output() => {
                res
            }
        };
        child.kill().await.ok();
        res
    }
}

#[async_trait]
impl Syncer for O365 {
    async fn discover(&self, state: &mut State) -> anyhow::Result<SyncColResult> {
        let id = self.col_id.clone();
        let auth_url = self.auth_url.clone();
        let props_path = self.props_path.clone();
        let log = self.log.clone();

        Self::with_davmail(&props_path, &id, auth_url.as_ref(), log, async || {
            let res = self.vdirsyncer.discover(state).await;
            self.remember_token(state, res).await
        })
        .await
    }

    async fn sync_cal(
        &mut self,
        state: &mut State,
        cal_id: &String,
    ) -> anyhow::Result<SyncColResult> {
        let id = self.col_id.clone();
        let auth_url = self.auth_url.clone();
        let props_path = self.props_path.clone();
        let log = self.log.clone();

        Self::with_davmail(&props_path, &id, auth_url.as_ref(), log, async || {
            let res = self.vdirsyncer.sync_cal(state, cal_id).await;
            self.remember_token(state, res).await
        })
        .await
    }

    async fn sync(&mut self, state: &mut State) -> anyhow::Result<SyncColResult> {
        let id = self.col_id.clone();
        let auth_url = self.auth_url.clone();
        let props_path = self.props_path.clone();
        let log = self.log.clone();

        Self::with_davmail(&props_path, &id, auth_url.as_ref(), log, async || {
            let res = self.vdirsyncer.sync(state).await;
            self.remember_token(state, res).await
        })
        .await
    }

    async fn delete_cal(&mut self, state: &mut State, cal_id: &String) -> anyhow::Result<()> {
        self.vdirsyncer.delete_cal(state, cal_id).await
    }

    async fn delete(&mut self, state: &mut State, config: bool) -> anyhow::Result<()> {
        self.vdirsyncer.delete(state, config).await?;

        if config {
            // remove generated property file
            fs::remove_file(&self.props_path).await.context(format!(
                "Deleting {} failed",
                self.props_path.to_str().unwrap()
            ))
        } else {
            Ok(())
        }
    }
}
