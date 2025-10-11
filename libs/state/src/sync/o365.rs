use anyhow::anyhow;
use async_trait::async_trait;
use std::io::Write;
use std::process::Stdio;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::EventixState;
use crate::sync::vdirsyncer::VDirSyncer;
use crate::sync::{SyncCalResult, Syncer};

pub struct O365 {
    vdirsyncer: VDirSyncer,
    davmail_cmd: Command,
    auth_url: Option<String>,
    props_file: NamedTempFile,
}

impl O365 {
    pub fn new(
        name: String,
        local_name: String,
        port: u16,
        user: String,
        auth_url: Option<&String>,
        token: Option<String>,
    ) -> anyhow::Result<Self> {
        let vdirsyncer = VDirSyncer::new(name, local_name);

        // create davmail command
        let mut davmail_cmd = Command::new("davmail");
        davmail_cmd.stdin(Stdio::piped());
        davmail_cmd.stdout(Stdio::piped());
        davmail_cmd.stderr(Stdio::piped());

        // generate properties file
        let props_file = Self::generate_props(port, user, token)?;
        let props_path = props_file.path().as_os_str().to_str().unwrap();

        davmail_cmd.args(&[props_path]);

        Ok(Self {
            vdirsyncer,
            davmail_cmd,
            auth_url: auth_url.cloned(),
            props_file,
        })
    }

    fn generate_props(
        port: u16,
        user: String,
        token: Option<String>,
    ) -> anyhow::Result<NamedTempFile> {
        let mut temp = NamedTempFile::new()?;
        writeln!(temp, "davmail.server=true")?;
        writeln!(temp, "davmail.mode=O365Manual")?;
        writeln!(temp, "davmail.enableOidc=true")?;
        writeln!(temp, "davmail.oauth.persistToken=true")?;
        writeln!(temp, "davmail.caldavPort={}", port)?;
        writeln!(temp, "davmail.allowRemote=false")?;
        writeln!(temp, "davmail.disableUpdateCheck=true")?;
        writeln!(temp, "davmail.enableKeepAlive=true")?;
        writeln!(temp, "davmail.folderSizeLimit=0")?;
        writeln!(temp, "davmail.defaultDomain=")?;
        writeln!(temp, "davmail.logFilePath=/dev/null")?;
        writeln!(temp, "log4j.logger.davmail=DEBUG")?;
        writeln!(temp, "davmail.disableGuiNotifications=true")?;
        writeln!(temp, "davmail.disableTrayActivitySwitch=true")?;
        writeln!(temp, "davmail.showStartupBanner=false")?;
        if let Some(token) = token {
            writeln!(temp, "davmail.oauth.{}.refreshToken={}", user, token)?;
        }
        temp.flush()?;
        Ok(temp)
    }

    async fn remember_token(&self, cal: &Arc<String>, state: &EventixState) -> anyhow::Result<()> {
        let file = File::options()
            .read(true)
            .open(self.props_file.path())
            .await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            // extract the token from the changed properties file
            if line.contains("refreshToken=") {
                if let Some(split) = line.find('=') {
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
        }
        Ok(())
    }
}

#[async_trait]
impl Syncer for O365 {
    async fn sync(
        &mut self,
        cal: &Arc<String>,
        state: EventixState,
    ) -> anyhow::Result<SyncCalResult> {
        let mut child = self.davmail_cmd.spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout).lines();

        // ensure that the server is started before we let vdirsyncer connect to it
        while let Ok(Some(line)) = reader.next_line().await {
            tracing::debug!("{}: {}", *cal, line);
            if line.contains("Start DavMail in server mode") {
                break;
            }
        }

        // read lines and watch for auth requests
        let mut read_output = async || {
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!("{}: {}", *cal, line);

                // do we need to (re-)authenticate?
                if line.starts_with("https://login.microsoftonline.com/") {
                    // if we already have the URL from the user, tell DavMail about it
                    if let Some(ref auth_url) = self.auth_url {
                        stdin.write(auth_url.as_bytes()).await?;
                        stdin.write(b"\n").await?;
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
            res = self.vdirsyncer.sync(cal, state.clone()) => {
                if let Ok(SyncCalResult::Success(_)) = res && self.auth_url.is_some() {
                    self.remember_token(cal, &state).await.ok();
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
