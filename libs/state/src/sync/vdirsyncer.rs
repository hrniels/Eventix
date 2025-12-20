use anyhow::{Context, anyhow};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::{process::Output, sync::Arc};
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use xdg::BaseDirectories;

use crate::State;
use crate::sync::{SyncColResult, Syncer, SyncerAuth};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SyncResult {
    Success(bool),
    NeedsDiscover,
}

enum EventType<'a> {
    Add(&'a str),
    Update(&'a str, &'a str),
    Delete(&'a str, &'a str),
}

#[derive(Default)]
struct Changes {
    added: bool,
    changed: Vec<String>,
    deleted: Vec<String>,
}

#[derive(Default)]
struct CalendarChanges {
    calendars: HashMap<String, Changes>,
}

impl CalendarChanges {
    fn handle_event<'a>(&mut self, ev: EventType<'a>, folder_id: &HashMap<String, String>) {
        let entry = match ev {
            EventType::Add(cal) | EventType::Update(_, cal) | EventType::Delete(_, cal) => {
                // all calendars are named "<id>_local/<folder>"
                let Some(sep) = cal.find("/") else {
                    return;
                };
                let Some(id) = folder_id.get(&cal[sep + 1..]) else {
                    return;
                };
                self.calendars.entry(id.clone()).or_default()
            }
        };
        match ev {
            EventType::Add(_) => entry.added = true,
            EventType::Update(uid, _) => entry.changed.push(uid.to_string()),
            EventType::Delete(uid, _) => entry.deleted.push(uid.to_string()),
        }
    }
}

pub struct VDirSyncer {
    name: String,
    folder_id: HashMap<String, String>,
    cfg: PathBuf,
}

impl VDirSyncer {
    pub async fn new(
        xdg: &BaseDirectories,
        name: String,
        folder_id: HashMap<String, String>,
        url: String,
        read_only: bool,
        auth: Option<SyncerAuth>,
    ) -> anyhow::Result<Self> {
        let cfg = Self::generate_config(xdg, &name, url, read_only, auth).await?;
        Ok(Self {
            name,
            folder_id,
            cfg,
        })
    }

    fn escape_value(val: &str) -> String {
        val.replace('"', "\\\"")
    }

    async fn generate_config(
        xdg: &BaseDirectories,
        name: &String,
        url: String,
        read_only: bool,
        auth: Option<SyncerAuth>,
    ) -> anyhow::Result<PathBuf> {
        let dir = xdg.get_data_file("vdirsyncer").unwrap();
        if !dir.exists() {
            fs::create_dir(&dir).await?;
        }

        let status_path = dir.join(format!("{}-status", name));
        let sync_path = dir.join(format!("{}-data", name));
        let cfg_path = dir.join(format!("{}.cfg", name));

        let name = Self::escape_value(name);
        let url = Self::escape_value(&url);

        let mut cfg = File::options()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&cfg_path)
            .await?;
        cfg.write_all(b"[general]\n").await?;
        cfg.write_all(format!("status_path = \"{}\"\n", status_path.to_str().unwrap()).as_bytes())
            .await?;

        // create the pair
        cfg.write_all(format!("[pair {}]\n", name).as_bytes())
            .await?;
        cfg.write_all(format!("a = \"{}_local\"\n", name).as_bytes())
            .await?;
        cfg.write_all(format!("b = \"{}_remote\"\n", name).as_bytes())
            .await?;
        cfg.write_all(b"collections = [\"from a\", \"from b\"]\n")
            .await?;
        cfg.write_all(b"metadata = [\"displayname\", \"color\"]\n")
            .await?;
        cfg.write_all(b"conflict_resolution = \"b wins\"\n").await?;

        // local storage
        cfg.write_all(format!("[storage {}_local]\n", name).as_bytes())
            .await?;
        cfg.write_all(b"type = \"filesystem\"\n").await?;
        cfg.write_all(b"fileext = \".ics\"\n").await?;
        cfg.write_all(format!("path = \"{}\"\n", sync_path.to_str().unwrap()).as_bytes())
            .await?;

        // remote storage
        cfg.write_all(format!("[storage {}_remote]\n", name).as_bytes())
            .await?;
        cfg.write_all(b"type = \"caldav\"\n").await?;
        cfg.write_all(format!("url = \"{}\"\n", url).as_bytes())
            .await?;
        cfg.write_all(format!("read_only = {}\n", read_only).as_bytes())
            .await?;
        if let Some(auth) = auth {
            cfg.write_all(
                format!("username = \"{}\"\n", Self::escape_value(&auth.user)).as_bytes(),
            )
            .await?;
            cfg.write_all(b"password.fetch = [\"command\"").await?;
            for comp in &auth.pw_cmd {
                cfg.write_all(format!(", \"{}\"", Self::escape_value(comp)).as_bytes())
                    .await?;
            }
            cfg.write_all(b"]\n").await?;
        } else {
            cfg.write_all(b"username = \"\"\n").await?;
            cfg.write_all(b"password = \"\"\n").await?;
        }

        Ok(cfg_path)
    }

    async fn run_discover(&self) -> anyhow::Result<()> {
        let mut cmd = Command::new("vdirsyncer");
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());
        cmd.args([
            "--config",
            self.cfg.to_str().unwrap(),
            "discover",
            &self.name,
        ]);

        let mut child = cmd.spawn()?;

        let mut stdin = child.stdin.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut stderr_reader = BufReader::new(stderr).lines();

        while let Some(line) = stderr_reader.next_line().await? {
            tracing::debug!("{}: {}", self.name, line);
            // in case it asks us whether to create the calendar, say "yes"
            stdin.write_all(b"y\n").await.unwrap();
        }

        let output = child.wait_with_output().await?;
        let status = output.status;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("discover failed: error code {:?}", status.code()))
        }
    }

    async fn run_sync(
        &mut self,
        state: &mut State,
        names: Vec<String>,
    ) -> anyhow::Result<SyncColResult> {
        let mut tried_discover = false;
        loop {
            let mut cmd = Command::new("vdirsyncer");
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            cmd.args(["--config", self.cfg.to_str().unwrap(), "sync"]);
            cmd.args(&names);

            let child = cmd.spawn()?;
            let output = child.wait_with_output().await?;
            let status = output.status;
            let res = self.post_process(state, output).await?;

            match res {
                SyncResult::NeedsDiscover => {
                    if tried_discover {
                        return Err(anyhow!("discover did not resolve sync error"));
                    }
                    self.run_discover().await?;
                    tried_discover = true;
                    continue;
                }
                SyncResult::Success(res) => {
                    if status.success() {
                        return Ok(SyncColResult::Success(res));
                    } else {
                        return Err(anyhow!("exited with {}", status));
                    }
                }
            }
        }
    }

    async fn post_process(&self, state: &mut State, output: Output) -> anyhow::Result<SyncResult> {
        let mut changes = CalendarChanges::default();

        for line in String::from_utf8(output.stderr)?.lines() {
            tracing::debug!("{}: {}", self.name, line);

            // vdirsyncer will complain if a collection changes and request a re-discover
            if line.contains("run `vdirsyncer discover") {
                return Ok(SyncResult::NeedsDiscover);
            }

            let w = line.split_whitespace().collect::<Vec<_>>();
            if w.len() < 5 {
                continue;
            }

            if let Some(ev) = match (w[0], w[1], w[2], w[3], w[4], w.get(5)) {
                ("Copying", "(uploading)", "item", _uid, "to", Some(cal)) => {
                    Some(EventType::Add(cal))
                }
                ("Copying", "(updating)", "item", uid, "to", Some(cal)) => {
                    Some(EventType::Update(uid, cal))
                }
                ("Deleting", "item", uid, "from", cal, _) => Some(EventType::Delete(uid, cal)),
                _ => None,
            } {
                changes.handle_event(ev, &self.folder_id);
            }
        }

        let seen_changes = changes
            .calendars
            .values()
            .any(|c| c.added || !c.changed.is_empty() || !c.deleted.is_empty());

        for (id, changes) in changes.calendars.iter() {
            if let Some(dir) = state.store_mut().directory_mut(&Arc::new(id.clone())) {
                if changes.added {
                    // rescan the whole directory for new files as we only know the new UIDs, but not
                    // necessarily their filenames (as these can be different).
                    dir.rescan_for_additions()?;
                }
                for uid in &changes.changed {
                    if let Some(file) = dir.file_by_id_mut(uid) {
                        file.reload_calendar()?;
                    } else {
                        tracing::warn!("file for uid {} does not exist", uid);
                    }
                }
                for uid in &changes.deleted {
                    dir.remove_by_uid(uid)?;
                }
            }
        }

        Ok(SyncResult::Success(seen_changes))
    }
}

#[async_trait]
impl Syncer for VDirSyncer {
    async fn discover(&self, _state: &mut State) -> anyhow::Result<SyncColResult> {
        self.run_discover().await?;

        let mut cmd = Command::new("vdirsyncer");
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());
        cmd.args([
            "--config",
            self.cfg.to_str().unwrap(),
            "metasync",
            &self.name,
        ]);

        let child = cmd.spawn()?;
        let output = child.wait_with_output().await?;
        for line in String::from_utf8(output.stderr)?.lines() {
            tracing::debug!("{}: {}", self.name, line);
        }

        if !output.status.success() {
            return Err(anyhow!("exited with {}", output.status));
        }
        Ok(SyncColResult::Success(false))
    }

    async fn sync_cal(
        &mut self,
        state: &mut State,
        cal_id: &String,
    ) -> anyhow::Result<SyncColResult> {
        let names = {
            let col = state.settings().collections().get(&self.name).unwrap();
            let (_, cal) = col
                .all_calendars()
                .iter()
                .find(|(id, _settings)| *id == cal_id)
                .ok_or_else(|| anyhow!("No calendar with id {}", cal_id))?;
            if !cal.enabled() {
                return Ok(SyncColResult::Success(false));
            }
            vec![format!("{}/{}", self.name, cal.folder())]
        };

        self.run_sync(state, names).await
    }

    async fn sync(&mut self, state: &mut State) -> anyhow::Result<SyncColResult> {
        // determine collection and pair names to sync
        let names = {
            let col = state.settings().collections().get(&self.name).unwrap();
            col.calendars()
                .map(|(_id, settings)| format!("{}/{}", &self.name, settings.folder()))
                .collect::<Vec<_>>()
        };
        if names.is_empty() {
            return Ok(SyncColResult::Success(false));
        }

        self.run_sync(state, names).await
    }

    async fn delete_cal(&mut self, state: &mut State, cal_id: &String) -> anyhow::Result<()> {
        let dir = self.cfg.parent().unwrap();

        let folder = state
            .settings()
            .collections()
            .get(&self.name)
            .unwrap()
            .all_calendars()
            .get(cal_id)
            .unwrap()
            .folder();

        // remove item in status directory
        let status_path = dir.join(format!("{}-status", self.name)).join(&self.name);
        for ext in [".items", ".metadata"] {
            let path = status_path.join(format!("{}{}", folder, ext));
            if path.exists() {
                fs::remove_file(&path)
                    .await
                    .context(format!("Removing {} failed", path.to_str().unwrap()))?;
            }
        }

        // remove all non-meta files in data directory
        let data_path = dir.join(format!("{}-data", self.name)).join(folder);
        let mut dir = fs::read_dir(data_path).await?;
        while let Some(entry) = dir.next_entry().await? {
            if entry.file_name() != "color" && entry.file_name() != "displayname" {
                fs::remove_file(entry.path()).await.context(format!(
                    "Removing {} failed",
                    entry.path().to_str().unwrap()
                ))?;
            }
        }

        Ok(())
    }

    async fn delete(&mut self, _state: &mut State, config: bool) -> anyhow::Result<()> {
        let dir = self.cfg.parent().unwrap();

        // remove complete status and directory
        for suffix in ["status", "data"] {
            let path = dir.join(format!("{}-{}", self.name, suffix));
            if path.exists() {
                fs::remove_dir_all(&path)
                    .await
                    .context(format!("Removing {} failed", path.to_str().unwrap()))?;
            }
        }

        if config {
            // remove generated config file
            fs::remove_file(&self.cfg)
                .await
                .context(format!("Removing {} failed", self.cfg.to_str().unwrap()))
        } else {
            Ok(())
        }
    }
}
