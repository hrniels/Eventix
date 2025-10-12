use anyhow::anyhow;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use xdg::BaseDirectories;

use crate::EventixState;
use crate::sync::vdirsyncer::VDirSyncer;
use crate::sync::{SyncCalResult, Syncer, SyncerAuth};

const PORT_BASE: u16 = 25000;

pub struct O365 {
    vdirsyncer: VDirSyncer,
    davmail_cmd: Command,
    auth_url: Option<String>,
    props_path: PathBuf,
}

impl O365 {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        xdg: &BaseDirectories,
        idx: usize,
        name: String,
        folder_id: HashMap<String, String>,
        read_only: bool,
        auth: SyncerAuth,
        auth_url: Option<&String>,
        token: Option<String>,
    ) -> anyhow::Result<Self> {
        let port = PORT_BASE + idx as u16;

        // generate properties file
        let props_path = Self::generate_props(xdg, &name, port, &auth.user, token).await?;

        // create vdirsyncer instance
        let url = format!("http://localhost:{}/users/{}/{}/", port, auth.user, name);
        let vdirsyncer = VDirSyncer::new(xdg, name, folder_id, url, read_only, Some(auth)).await?;

        // create davmail command
        let mut davmail_cmd = Command::new("davmail");
        davmail_cmd.stdin(Stdio::piped());
        davmail_cmd.stdout(Stdio::piped());
        davmail_cmd.stderr(Stdio::piped());
        davmail_cmd.args([props_path.to_str().unwrap()]);

        Ok(Self {
            vdirsyncer,
            davmail_cmd,
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

    async fn remember_token(&self, cal: &Arc<String>, state: &EventixState) -> anyhow::Result<()> {
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
                misc.set_calendar_token(cal, token.to_string());
                if let Err(e) = misc.write_to_file() {
                    tracing::warn!("Unable to save misc state: {}", e);
                }
                break;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Syncer for O365 {
    async fn sync(
        &mut self,
        col: &Arc<String>,
        state: EventixState,
    ) -> anyhow::Result<SyncCalResult> {
        let mut child = self.davmail_cmd.spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout).lines();

        // ensure that the server is started before we let vdirsyncer connect to it
        while let Ok(Some(line)) = reader.next_line().await {
            tracing::debug!("{}: {}", *col, line);
            if line.contains("Start DavMail in server mode") {
                break;
            }
        }

        // read lines and watch for auth requests
        let mut read_output = async || {
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!("{}: {}", *col, line);

                // do we need to (re-)authenticate?
                if line.starts_with("https://login.microsoftonline.com/") {
                    // if we already have the URL from the user, tell DavMail about it
                    if let Some(ref auth_url) = self.auth_url {
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
            // wait until sync finished
            res = self.vdirsyncer.sync(col, state.clone()) => {
                if let Ok(SyncCalResult::Success(_)) = res && self.auth_url.is_some() {
                    self.remember_token(col, &state).await.ok();
                }
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
