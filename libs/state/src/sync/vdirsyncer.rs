use anyhow::anyhow;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::{process::Output, sync::Arc};
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use xdg::BaseDirectories;

use crate::EventixState;
use crate::sync::{SyncCalResult, Syncer};

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
                self.calendars
                    .entry(id.clone())
                    .or_insert(Changes::default())
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
    cmd: Command,
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
        user: &String,
        pw_cmd: Vec<String>,
    ) -> anyhow::Result<Self> {
        let cfg = Self::generate_config(xdg, &name, url, read_only, user, pw_cmd).await?;
        let mut cmd = Command::new("vdirsyncer");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.args(&["--config", cfg.to_str().unwrap(), "sync", &name]);
        Ok(Self {
            cmd,
            name,
            folder_id,
            cfg,
        })
    }

    async fn generate_config(
        xdg: &BaseDirectories,
        name: &String,
        url: String,
        read_only: bool,
        user: &String,
        pw_cmd: Vec<String>,
    ) -> anyhow::Result<PathBuf> {
        let dir = xdg.get_data_file("vdirsyncer").unwrap();
        if !dir.exists() {
            fs::create_dir(&dir).await?;
        }

        let status_path = dir.join(format!("{}-status", name));
        let sync_path = dir.join(format!("{}-data", name));
        let cfg_path = dir.join(format!("{}.cfg", name));

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
        cfg.write_all(format!("username = \"{}\"\n", user).as_bytes())
            .await?;
        cfg.write_all(b"password.fetch = [\"command\"").await?;
        for comp in &pw_cmd {
            cfg.write_all(format!(", \"{}\"", comp).as_bytes()).await?;
        }
        cfg.write_all(b"]\n").await?;

        Ok(cfg_path)
    }

    async fn discover(&self, cal: &Arc<String>) -> anyhow::Result<()> {
        let mut cmd = Command::new("vdirsyncer");
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.args(&[
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
            tracing::debug!("{}: {}", *cal, line);
            // in case it asks us whether to create the calendar, say "yes"
            stdin.write(b"y\n").await.unwrap();
        }

        let output = child.wait_with_output().await?;
        let status = output.status;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("discover failed: error code {:?}", status.code()))
        }
    }

    async fn post_process(
        &self,
        cal: &Arc<String>,
        state: &EventixState,
        output: Output,
    ) -> anyhow::Result<SyncResult> {
        let mut changes = CalendarChanges::default();

        for line in String::from_utf8(output.stderr)?.lines() {
            tracing::debug!("{}: {}", *cal, line);

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

        let mut state = state.lock().await;
        for (id, changes) in changes.calendars.iter() {
            if let Some(dir) = state.store_mut().directory_mut(&Arc::new(id.clone())) {
                if changes.added {
                    // rescan the whole directory for new files as we only know the new UIDs, but not
                    // necessarily their filenames (as these can be different).
                    dir.rescan_for_additions()?;
                }
                for uid in &changes.changed {
                    if let Some(file) = dir.file_by_id_mut(&uid) {
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
    async fn sync(
        &mut self,
        col: &Arc<String>,
        state: EventixState,
    ) -> anyhow::Result<SyncCalResult> {
        let mut tried_discover = false;
        loop {
            let child = self.cmd.spawn()?;
            let output = child.wait_with_output().await?;
            let status = output.status;
            let res = self.post_process(col, &state, output).await?;

            match res {
                SyncResult::NeedsDiscover => {
                    if tried_discover {
                        return Err(anyhow!("discover did not resolve sync error"));
                    }
                    self.discover(col).await?;
                    tried_discover = true;
                    continue;
                }
                SyncResult::Success(res) => {
                    if status.success() {
                        return Ok(SyncCalResult::Success(res));
                    } else {
                        return Err(anyhow!("exited with {}", status));
                    }
                }
            }
        }
    }
}
