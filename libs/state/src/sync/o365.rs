use anyhow::{Context, anyhow};
use async_trait::async_trait;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use xdg::BaseDirectories;

use crate::EventixState;
use crate::sync::vdirsyncer::VDirSyncer;
use crate::sync::{SyncCalResult, Syncer, SyncerAuth};

const PORT_BASE: u16 = 25000;

pub struct O365 {
    col_id: String,
    vdirsyncer: VDirSyncer,
    auth_url: Option<String>,
    props_path: PathBuf,
}

impl O365 {
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
    ) -> anyhow::Result<Self> {
        let port = PORT_BASE + idx as u16;

        // generate properties file
        let props_path = Self::generate_props(xdg, &col_id, port, &auth.user, token).await?;

        // build URL
        let user_enc = utf8_percent_encode(&auth.user, NON_ALPHANUMERIC).to_string();
        let col_enc = utf8_percent_encode(&col_id, NON_ALPHANUMERIC).to_string();
        let url = format!("http://localhost:{}/users/{}/{}/", port, user_enc, col_enc);

        // create vdirsyncer instance
        let vdirsyncer =
            VDirSyncer::new(xdg, col_id.clone(), folder_id, url, read_only, Some(auth)).await?;

        Ok(Self {
            col_id,
            vdirsyncer,
            auth_url: auth_url.cloned(),
            props_path,
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
        if !dir.exists() {
            fs::create_dir(&dir).await?;
        }

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
        state: &EventixState,
        res: anyhow::Result<SyncCalResult>,
    ) -> anyhow::Result<SyncCalResult> {
        if let Ok(SyncCalResult::Success(_)) = res
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
                    let mut state = state.lock().await;
                    let misc = state.misc_mut();
                    misc.set_calendar_token(&self.col_id, token.to_string());
                    misc.write_to_file()?;
                    break;
                }
            }
        }
        res
    }

    async fn with_davmail<F, Fut>(
        props_path: &Path,
        id: &String,
        auth_url: Option<&String>,
        func: F,
    ) -> anyhow::Result<SyncCalResult>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<SyncCalResult>>,
    {
        let mut cmd = Command::new("davmail");
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.args([props_path.to_str().unwrap()]);

        let mut child = cmd.spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout).lines();

        // ensure that the server is started before we let vdirsyncer connect to it
        while let Ok(Some(line)) = reader.next_line().await {
            tracing::debug!("{}: {}", id, line);
            if line.contains("Start DavMail in server mode") {
                break;
            }
        }

        // read lines and watch for auth requests
        let mut read_output = async || {
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!("{}: {}", id, line);

                // do we need to (re-)authenticate?
                if line.starts_with("https://login.microsoftonline.com/") {
                    // if we already have the URL from the user, tell DavMail about it
                    if let Some(auth_url) = auth_url {
                        stdin.write_all(auth_url.as_bytes()).await?;
                        stdin.write_all(b"\n").await?;
                    } else {
                        // otherwise we fail and ask the user to authenticate
                        return Ok(SyncCalResult::AuthFailed(line));
                    }
                }
            }
            Err(anyhow!("DavMail exited first"))
        };

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
    async fn discover(&self, state: EventixState) -> anyhow::Result<SyncCalResult> {
        let id = self.col_id.clone();
        let auth_url = self.auth_url.clone();
        let props_path = self.props_path.clone();

        Self::with_davmail(&props_path, &id, auth_url.as_ref(), async || {
            let res = self.vdirsyncer.discover(state.clone()).await;
            self.remember_token(&state, res).await
        })
        .await
    }

    async fn sync_cal(
        &mut self,
        state: EventixState,
        cal_id: &String,
    ) -> anyhow::Result<SyncCalResult> {
        let id = self.col_id.clone();
        let auth_url = self.auth_url.clone();
        let props_path = self.props_path.clone();

        Self::with_davmail(&props_path, &id, auth_url.as_ref(), async || {
            let res = self.vdirsyncer.sync_cal(state.clone(), cal_id).await;
            self.remember_token(&state, res).await
        })
        .await
    }

    async fn sync(&mut self, state: EventixState) -> anyhow::Result<SyncCalResult> {
        let id = self.col_id.clone();
        let auth_url = self.auth_url.clone();
        let props_path = self.props_path.clone();

        Self::with_davmail(&props_path, &id, auth_url.as_ref(), async || {
            let res = self.vdirsyncer.sync(state.clone()).await;
            self.remember_token(&state, res).await
        })
        .await
    }

    async fn delete_cal(&mut self, state: EventixState, cal_id: &String) -> anyhow::Result<()> {
        self.vdirsyncer.delete_cal(state, cal_id).await
    }

    async fn delete(&mut self, state: EventixState, config: bool) -> anyhow::Result<()> {
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
