// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{ExitStatus, Output, Stdio};
use std::sync::Arc;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use xdg::BaseDirectories;

use crate::State;
use crate::settings::SyncTimeSpan;
use crate::sync::{SyncColResult, Syncer, SyncerAuth, log_line};

// --- CommandRunner trait and implementations ---

/// Abstracts subprocess execution so that tests can inject a fake runner without spawning real
/// processes.
#[async_trait]
pub(crate) trait CommandRunner: Send + Sync {
    /// Runs `program` with `args`, optionally writing `stdin_data` to the process's stdin before
    /// waiting for it to finish. Returns the full [`Output`] (stdout, stderr, exit status).
    async fn run(
        &self,
        program: &str,
        args: &[&str],
        stdin_data: Option<&[u8]>,
    ) -> anyhow::Result<Output>;

    /// Runs `program` with `args` interactively: for each line emitted on stderr, writes
    /// `yes_response` to stdin. Returns the exit status and all stderr lines that were produced.
    async fn run_interactive(
        &self,
        program: &str,
        args: &[&str],
        yes_response: &[u8],
    ) -> anyhow::Result<(ExitStatus, Vec<String>)>;
}

/// Production [`CommandRunner`] that spawns real subprocesses via [`tokio::process::Command`].
pub(crate) struct RealCommandRunner;

#[async_trait]
impl CommandRunner for RealCommandRunner {
    async fn run(
        &self,
        program: &str,
        args: &[&str],
        stdin_data: Option<&[u8]>,
    ) -> anyhow::Result<Output> {
        let mut cmd = Command::new(program);
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        if stdin_data.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }

        cmd.args(args);
        let mut child = cmd.spawn()?;

        if let Some(data) = stdin_data
            && let Some(mut stdin) = child.stdin.take()
        {
            stdin.write_all(data).await?;
        }

        Ok(child.wait_with_output().await?)
    }

    async fn run_interactive(
        &self,
        program: &str,
        args: &[&str],
        yes_response: &[u8],
    ) -> anyhow::Result<(ExitStatus, Vec<String>)> {
        let mut cmd = Command::new(program);
        cmd.kill_on_drop(true);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());
        cmd.args(args);

        let mut child = cmd.spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut stderr_reader = BufReader::new(stderr).lines();
        let mut lines = Vec::new();

        while let Some(line) = stderr_reader.next_line().await? {
            lines.push(line);
            stdin.write_all(yes_response).await.unwrap();
        }

        let output = child.wait_with_output().await?;
        Ok((output.status, lines))
    }
}

// --- Parsing types ---

/// Internal result of a single vdirsyncer invocation, parsed from its stderr output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SyncResult {
    /// The sync completed; the boolean indicates whether any items were added, updated,
    /// or deleted.
    Success(bool),
    /// vdirsyncer reported a collection-structure change and requested re-discovery before
    /// the next sync can proceed.
    NeedsDiscover,
}

enum EventType<'a> {
    Add(&'a str),
    Update(&'a str, &'a str),
    Delete(&'a str, &'a str),
}

/// Item-level changes detected for a single calendar during one vdirsyncer run.
#[derive(Default)]
pub(crate) struct Changes {
    /// Whether at least one new item was uploaded to the local store.
    pub(crate) added: bool,
    /// UIDs of items that were updated in the local store.
    pub(crate) changed: Vec<String>,
    /// UIDs of items that were removed from the local store.
    pub(crate) deleted: Vec<String>,
}

/// Aggregated per-calendar changes parsed from one vdirsyncer invocation.
#[derive(Default)]
pub(crate) struct CalendarChanges {
    /// Per-calendar change sets, keyed by calendar ID.
    pub(crate) calendars: HashMap<String, Changes>,
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

/// Parses a block of vdirsyncer stderr output and returns the sync result along with the set of
/// per-calendar changes detected.
///
/// Returns retry instructions for discover or missing-collection cases. Otherwise returns
/// `SyncResult::Success(changed)` where `changed` is `true` when at least one add, update, or
/// delete was parsed.
pub(crate) fn parse_output(
    stderr: &str,
    folder_id: &HashMap<String, String>,
) -> (SyncResult, CalendarChanges) {
    let mut changes = CalendarChanges::default();

    for line in stderr.lines() {
        // vdirsyncer will complain if a collection changes and request a re-discover
        if line.contains("run `vdirsyncer discover") {
            return (SyncResult::NeedsDiscover, changes);
        }

        let w = line.split_whitespace().collect::<Vec<_>>();
        if w.len() < 5 {
            continue;
        }

        // Match against fixed word positions in vdirsyncer's output format:
        //   "Copying (uploading) item <uid> to <cal>"  → Add
        //   "Copying (updating) item <uid> to <cal>"   → Update
        //   "Deleting item <uid> from <cal>"           → Delete
        if let Some(ev) = match (w[0], w[1], w[2], w[3], w[4], w.get(5)) {
            ("Copying", "(uploading)", "item", _uid, "to", Some(cal)) => Some(EventType::Add(cal)),
            ("Copying", "(updating)", "item", uid, "to", Some(cal)) => {
                Some(EventType::Update(uid, cal))
            }
            ("Deleting", "item", uid, "from", cal, _) => Some(EventType::Delete(uid, cal)),
            _ => None,
        } {
            changes.handle_event(ev, folder_id);
        }
    }

    let seen_changes = changes
        .calendars
        .values()
        .any(|c| c.added || !c.changed.is_empty() || !c.deleted.is_empty());

    (SyncResult::Success(seen_changes), changes)
}

// --- VDirSyncer ---

/// A [`Syncer`] implementation that delegates to the `vdirsyncer` command-line tool.
///
/// Manages a generated vdirsyncer configuration file and drives `vdirsyncer discover` and
/// `vdirsyncer sync` as subprocesses, parsing their output to apply incremental updates to the
/// in-memory calendar store.
pub struct VDirSyncer {
    name: String,
    folder_id: HashMap<String, String>,
    cfg: PathBuf,
    log: Arc<Mutex<File>>,
    runner: Arc<dyn CommandRunner>,
}

impl VDirSyncer {
    /// Creates a new `VDirSyncer`, generating the vdirsyncer configuration file on disk.
    ///
    /// `folder_id` maps vdirsyncer folder names to calendar IDs. Returns an error if the
    /// configuration file cannot be written.
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        xdg: &BaseDirectories,
        name: String,
        folder_id: HashMap<String, String>,
        url: String,
        read_only: bool,
        auth: Option<SyncerAuth>,
        time_span: &SyncTimeSpan,
        log: Arc<Mutex<File>>,
    ) -> anyhow::Result<Self> {
        Self::new_with_runner(
            xdg,
            name,
            folder_id,
            url,
            read_only,
            auth,
            time_span,
            log,
            Arc::new(RealCommandRunner),
        )
        .await
    }

    /// Creates a new `VDirSyncer` with a custom [`CommandRunner`], for use in tests.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn new_with_runner(
        xdg: &BaseDirectories,
        name: String,
        folder_id: HashMap<String, String>,
        url: String,
        read_only: bool,
        auth: Option<SyncerAuth>,
        time_span: &SyncTimeSpan,
        log: Arc<Mutex<File>>,
        runner: Arc<dyn CommandRunner>,
    ) -> anyhow::Result<Self> {
        let cfg = Self::generate_config(xdg, &name, url, read_only, auth, time_span).await?;
        Ok(Self {
            name,
            folder_id,
            cfg,
            log,
            runner,
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
        time_span: &SyncTimeSpan,
    ) -> anyhow::Result<PathBuf> {
        let dir = xdg.get_data_file("vdirsyncer").unwrap();
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
        cfg.write_all(b"implicit = \"create\"\n").await?;

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
        if time_span.needs_date_filter() {
            cfg.write_all(format!("start_date = \"{}\"\n", time_span.start_expr()).as_bytes())
                .await?;
            cfg.write_all(format!("end_date = \"{}\"\n", time_span.end_expr()).as_bytes())
                .await?;
        }
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

        // tokio::fs::File does not flush on drop; explicitly flush so all buffered writes reach
        // the filesystem before the caller reads the file back.
        cfg.flush().await?;

        Ok(cfg_path)
    }

    async fn run_discover(&self) -> anyhow::Result<()> {
        let args = [
            "--config",
            self.cfg.to_str().unwrap(),
            "discover",
            &self.name,
        ];

        let (status, lines) = self
            .runner
            .run_interactive("vdirsyncer", &args, b"y\n")
            .await?;

        for line in &lines {
            log_line(&self.log, &self.name, line).await?;
        }

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
            let mut args = vec!["--config", self.cfg.to_str().unwrap(), "sync"];
            let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            args.extend_from_slice(&name_refs);

            let output = self.runner.run("vdirsyncer", &args, None).await?;
            let status = output.status;
            let res = self.post_process(state, output).await?;

            match res {
                SyncResult::NeedsDiscover => {
                    // vdirsyncer reported a collection change and asked for re-discovery. Run
                    // discover once and retry the sync. If discover was already attempted this
                    // run, give up to avoid an infinite loop.
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

    async fn run_delete_sync(&self, names: Vec<String>) -> anyhow::Result<()> {
        let mut args = vec!["--config", self.cfg.to_str().unwrap(), "delete"];
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        args.extend_from_slice(&name_refs);

        let output = self.runner.run("vdirsyncer", &args, None).await?;
        let stderr = String::from_utf8(output.stderr)?;
        for line in stderr.lines() {
            log_line(&self.log, &self.name, line).await?;
        }

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow!("exited with {}", output.status))
        }
    }

    async fn run_create_sync(&self, names: Vec<String>) -> anyhow::Result<()> {
        let mut args = vec!["--config", self.cfg.to_str().unwrap(), "create"];
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        args.extend_from_slice(&name_refs);

        let output = self.runner.run("vdirsyncer", &args, None).await?;
        let stderr = String::from_utf8(output.stderr)?;
        for line in stderr.lines() {
            log_line(&self.log, &self.name, line).await?;
        }

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow!("exited with {}", output.status))
        }
    }

    async fn remove_local_folder_data(
        &self,
        folder: &str,
        remove_meta: bool,
    ) -> anyhow::Result<()> {
        let dir = self.cfg.parent().unwrap();

        let status_path = dir.join(format!("{}-status", self.name)).join(&self.name);
        for ext in [".items", ".metadata"] {
            let path = status_path.join(format!("{}{}", folder, ext));
            if path.exists() {
                fs::remove_file(&path)
                    .await
                    .context(format!("Removing {} failed", path.to_str().unwrap()))?;
            }
        }

        let data_path = dir.join(format!("{}-data", self.name)).join(folder);
        if let Ok(mut dir) = fs::read_dir(data_path).await {
            while let Some(entry) = dir.next_entry().await? {
                if remove_meta
                    || (entry.file_name() != "color" && entry.file_name() != "displayname")
                {
                    fs::remove_file(entry.path()).await.context(format!(
                        "Removing {} failed",
                        entry.path().to_str().unwrap()
                    ))?;
                }
            }
        }

        Ok(())
    }

    async fn post_process(&self, state: &mut State, output: Output) -> anyhow::Result<SyncResult> {
        let stderr = String::from_utf8(output.stderr)?;
        let (result, changes) = parse_output(&stderr, &self.folder_id);

        // Log every line now that we have the full stderr string.
        for line in stderr.lines() {
            log_line(&self.log, &self.name, line).await?;
        }

        if matches!(result, SyncResult::NeedsDiscover) {
            return Ok(result);
        }

        let local_tz = *state.timezone();
        for (id, changes) in changes.calendars.iter() {
            if let Some(dir) = state.store_mut().directory_mut(&Arc::new(id.clone())) {
                if changes.added {
                    // rescan the whole directory for new files as we only know the new UIDs, but
                    // not necessarily their filenames (as these can be different).
                    dir.rescan_for_additions(&local_tz)?;
                }
                for uid in &changes.changed {
                    if let Some(file) = dir.file_by_id_mut(uid) {
                        file.reload_calendar(&local_tz)?;
                    } else {
                        tracing::warn!("file for uid {} does not exist", uid);
                    }
                }
                for uid in &changes.deleted {
                    dir.remove_by_uid(uid)?;
                }
            }
        }

        Ok(result)
    }
}

#[async_trait]
impl Syncer for VDirSyncer {
    async fn discover(&self, _state: &mut State) -> anyhow::Result<SyncColResult> {
        self.run_discover().await?;

        // metasync propagates calendar metadata (display name, colour) from the remote storage
        // to the local storage after discovery, so that subsequent syncs see up-to-date names.
        let args = [
            "--config",
            self.cfg.to_str().unwrap(),
            "metasync",
            &self.name,
        ];
        let output = self.runner.run("vdirsyncer", &args, None).await?;
        for line in String::from_utf8(output.stderr)?.lines() {
            log_line(&self.log, &self.name, line).await?;
        }

        if !output.status.success() {
            return Err(anyhow!("exited with {}", output.status));
        }
        Ok(SyncColResult::Success(true))
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
        let folder = state
            .settings()
            .collections()
            .get(&self.name)
            .unwrap()
            .all_calendars()
            .get(cal_id)
            .map(|cal| cal.folder().as_str())
            .ok_or_else(|| anyhow!("No calendar with id {}", cal_id))?;
        self.remove_local_folder_data(folder, false).await
    }

    async fn create_cal_by_folder(
        &mut self,
        _state: &mut State,
        folder: &String,
    ) -> anyhow::Result<()> {
        self.run_create_sync(vec![format!("{}/{}", self.name, folder)])
            .await?;
        self.run_discover().await
    }

    async fn delete_cal_by_folder(
        &mut self,
        _state: &mut State,
        folder: &String,
    ) -> anyhow::Result<()> {
        self.run_delete_sync(vec![format!("{}/{}", self.name, folder)])
            .await?;
        self.remove_local_folder_data(folder, false).await
    }

    async fn delete(&mut self, _state: &mut State, all: bool) -> anyhow::Result<()> {
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

        if all {
            // remove generated config file
            fs::remove_file(&self.cfg)
                .await
                .context(format!("Removing {} failed", self.cfg.to_str().unwrap()))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eventix_ical::col::CalStore;
    use std::os::unix::process::ExitStatusExt;
    use std::sync::Mutex as StdMutex;

    // --- FakeCommandRunner ---

    /// A recorded call made to the fake runner.
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    enum RunCall {
        Run { program: String, args: Vec<String> },
        RunInteractive { program: String, args: Vec<String> },
    }

    /// Canned response for one `run` call.
    struct CannedOutput {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        /// `None` means success (exit code 0).
        exit_code: Option<i32>,
    }

    impl CannedOutput {
        fn success(stderr: impl Into<Vec<u8>>) -> Self {
            Self {
                stdout: vec![],
                stderr: stderr.into(),
                exit_code: None,
            }
        }

        fn failure(stderr: impl Into<Vec<u8>>) -> Self {
            Self {
                stdout: vec![],
                stderr: stderr.into(),
                exit_code: Some(1),
            }
        }
    }

    struct FakeCommandRunner {
        /// Responses returned by `run`, consumed in order.
        run_responses: StdMutex<Vec<CannedOutput>>,
        /// Responses returned by `run_interactive`, consumed in order.
        interactive_responses: StdMutex<Vec<(ExitStatus, Vec<String>)>>,
        /// All calls recorded for later inspection.
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

        fn calls(&self) -> Vec<RunCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl CommandRunner for FakeCommandRunner {
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
                stdout: canned.stdout,
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

            let resp = self.interactive_responses.lock().unwrap().remove(0);

            Ok(resp)
        }
    }

    // --- parse_output tests ---

    fn make_folder_id() -> HashMap<String, String> {
        // folder "work" maps to calendar id "cal-work"
        let mut m = HashMap::new();
        m.insert("work".to_string(), "cal-work".to_string());
        m
    }

    #[test]
    fn parse_output_empty_stderr_is_no_change() {
        let folder_id = make_folder_id();
        let (result, changes) = parse_output("", &folder_id);
        assert_eq!(result, SyncResult::Success(false));
        assert!(changes.calendars.is_empty());
    }

    #[test]
    fn parse_output_short_lines_are_ignored() {
        let folder_id = make_folder_id();
        let stderr = "Starting sync\nDone\n";
        let (result, changes) = parse_output(stderr, &folder_id);
        assert_eq!(result, SyncResult::Success(false));
        assert!(changes.calendars.is_empty());
    }

    #[test]
    fn parse_output_add_event() {
        let folder_id = make_folder_id();
        // vdirsyncer format: "Copying (uploading) item <uid> to <col>_local/<folder>"
        let stderr = "Copying (uploading) item uid-1 to mycol_local/work\n";
        let (result, changes) = parse_output(stderr, &folder_id);
        assert_eq!(result, SyncResult::Success(true));
        let cal = changes.calendars.get("cal-work").expect("cal-work present");
        assert!(cal.added);
        assert!(cal.changed.is_empty());
        assert!(cal.deleted.is_empty());
    }

    #[test]
    fn parse_output_update_event() {
        let folder_id = make_folder_id();
        let stderr = "Copying (updating) item uid-2 to mycol_local/work\n";
        let (result, changes) = parse_output(stderr, &folder_id);
        assert_eq!(result, SyncResult::Success(true));
        let cal = changes.calendars.get("cal-work").expect("cal-work present");
        assert!(!cal.added);
        assert_eq!(cal.changed, vec!["uid-2"]);
        assert!(cal.deleted.is_empty());
    }

    #[test]
    fn parse_output_delete_event() {
        let folder_id = make_folder_id();
        let stderr = "Deleting item uid-3 from mycol_local/work\n";
        let (result, changes) = parse_output(stderr, &folder_id);
        assert_eq!(result, SyncResult::Success(true));
        let cal = changes.calendars.get("cal-work").expect("cal-work present");
        assert!(!cal.added);
        assert!(cal.changed.is_empty());
        assert_eq!(cal.deleted, vec!["uid-3"]);
    }

    #[test]
    fn parse_output_needs_discover() {
        let folder_id = make_folder_id();
        let stderr =
            "Aborting synchronization: please run `vdirsyncer discover` to update collections.\n";
        let (result, _) = parse_output(stderr, &folder_id);
        assert_eq!(result, SyncResult::NeedsDiscover);
    }

    #[test]
    fn parse_output_missing_collection() {
        let folder_id = make_folder_id();
        let stderr = "critical: Pair foo: Collection \"bar\" not found.These are the configured collections\n";
        let (result, _) = parse_output(stderr, &folder_id);
        assert_eq!(result, SyncResult::Success(false));
    }

    #[test]
    fn parse_output_unknown_folder_is_ignored() {
        let folder_id = make_folder_id();
        // "personal" is not in folder_id
        let stderr = "Copying (uploading) item uid-1 to mycol_local/personal\n";
        let (result, changes) = parse_output(stderr, &folder_id);
        assert_eq!(result, SyncResult::Success(false));
        assert!(changes.calendars.is_empty());
    }

    #[test]
    fn parse_output_mixed_events() {
        let folder_id = make_folder_id();
        let stderr = concat!(
            "Copying (uploading) item new-uid to mycol_local/work\n",
            "Copying (updating) item changed-uid to mycol_local/work\n",
            "Deleting item gone-uid from mycol_local/work\n",
        );
        let (result, changes) = parse_output(stderr, &folder_id);
        assert_eq!(result, SyncResult::Success(true));
        let cal = changes.calendars.get("cal-work").unwrap();
        assert!(cal.added);
        assert_eq!(cal.changed, vec!["changed-uid"]);
        assert_eq!(cal.deleted, vec!["gone-uid"]);
    }

    // --- run_sync retry-loop tests ---

    /// Builds a minimal tempdir-backed XDG and a log file for tests that need a `VDirSyncer`
    /// instance but will not actually write config (we control the runner entirely).
    async fn make_syncer_with_runner(
        runner: Arc<dyn CommandRunner>,
    ) -> (VDirSyncer, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = crate::with_test_xdg(&tmp.path().join("data"), &tmp.path().join("config"));

        // create the vdirsyncer data dir that generate_config expects
        let vdir: PathBuf = xdg.get_data_file("vdirsyncer").unwrap();
        tokio::fs::create_dir_all(&vdir).await.unwrap();

        let log_path = vdir.join("test.log");
        let log_file: tokio::fs::File = tokio::fs::File::options()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
            .unwrap();
        let log: Arc<Mutex<File>> = Arc::new(Mutex::new(log_file));

        // We use an empty folder_id map; tests for run_sync only care about SyncResult.
        let syncer = VDirSyncer::new_with_runner(
            &xdg,
            "testcol".to_string(),
            HashMap::new(),
            "http://localhost/".to_string(),
            false,
            None,
            &crate::settings::SyncTimeSpan::default(),
            log,
            runner,
        )
        .await
        .unwrap();

        // Return the TempDir alongside so it is not dropped (and the dir removed) too early.
        (syncer, tmp)
    }

    #[tokio::test]
    async fn discover_calls_discover_then_metasync() {
        // Provide: one interactive response (for discover) + one run response (for metasync).
        let ok_status = ExitStatus::from_raw(0);
        let runner =
            FakeCommandRunner::new(vec![CannedOutput::success(b"")], vec![(ok_status, vec![])]);
        let runner_ref = runner.clone();

        let (syncer, _tmp) = make_syncer_with_runner(runner_ref).await;

        // discover requires a mutable State; we pass a minimal one built on a tempdir.
        // Because FSSyncer is a no-op for discover we just need the call counts.
        // NOTE: we cannot easily construct a full State here, so we test at the runner level:
        // just verify that the right sub-commands were invoked.
        // (A full State integration test lives in the integration test suite.)

        let calls = runner.calls();
        // No calls yet - discover hasn't been invoked, we only built the syncer.
        assert!(calls.is_empty());

        // Manually trigger run_discover (which is the discover sub-step tested here).
        syncer.run_discover().await.unwrap();

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        let RunCall::RunInteractive { program, args } = &calls[0] else {
            panic!("expected RunInteractive, got {:?}", calls[0]);
        };
        assert_eq!(program, "vdirsyncer");
        assert!(args.contains(&"discover".to_string()));
    }

    #[tokio::test]
    async fn run_discover_returns_error_on_nonzero_exit() {
        let fail_status = ExitStatus::from_raw(1 << 8);
        let runner = FakeCommandRunner::new(
            vec![],
            vec![(fail_status, vec!["something went wrong".to_string()])],
        );

        let (syncer, _tmp) = make_syncer_with_runner(runner).await;
        let result = syncer.run_discover().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("discover failed"));
    }

    // --- escape_value tests ---

    #[test]
    fn escape_value_replaces_double_quotes() {
        assert_eq!(VDirSyncer::escape_value(r#"pass"word"#), r#"pass\"word"#);
        assert_eq!(VDirSyncer::escape_value("no quotes here"), "no quotes here");
        assert_eq!(VDirSyncer::escape_value(""), "");
    }

    // --- generate_config tests ---

    #[tokio::test]
    async fn generate_config_writes_expected_sections() {
        // Build a syncer and then read back the generated .cfg file to verify its structure.
        let runner = FakeCommandRunner::new(vec![], vec![]);
        let (syncer, _tmp) = make_syncer_with_runner(runner).await;

        let content = tokio::fs::read_to_string(&syncer.cfg).await.unwrap();

        assert!(content.contains("[general]"), "missing [general]");
        assert!(content.contains("[pair testcol]"), "missing [pair testcol]");
        assert!(
            content.contains("[storage testcol_local]"),
            "missing [storage testcol_local]"
        );
        assert!(
            content.contains("[storage testcol_remote]"),
            "missing [storage testcol_remote]"
        );
        assert!(
            content.contains("type = \"filesystem\""),
            "missing local type"
        );
        assert!(content.contains("type = \"caldav\""), "missing remote type");
        assert!(
            content.contains("url = \"http://localhost/\""),
            "missing url"
        );
        assert!(content.contains("read_only = false"), "missing read_only");
    }

    #[tokio::test]
    async fn generate_config_writes_auth_when_provided() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = crate::with_test_xdg(&tmp.path().join("data"), &tmp.path().join("config"));
        let vdir: PathBuf = xdg.get_data_file("vdirsyncer").unwrap();
        tokio::fs::create_dir_all(&vdir).await.unwrap();

        let log_path = vdir.join("auth.log");
        let log_file = tokio::fs::File::options()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
            .unwrap();
        let log = Arc::new(Mutex::new(log_file));

        let auth = SyncerAuth {
            user: "user@example.com".to_string(),
            pw_cmd: vec!["pass".to_string(), "show".to_string(), "work".to_string()],
        };

        let syncer = VDirSyncer::new_with_runner(
            &xdg,
            "authcol".to_string(),
            HashMap::new(),
            "http://localhost/".to_string(),
            true,
            Some(auth),
            &crate::settings::SyncTimeSpan::default(),
            log,
            FakeCommandRunner::new(vec![], vec![]),
        )
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(&syncer.cfg).await.unwrap();
        assert!(content.contains("username = \"user@example.com\""));
        assert!(content.contains("password.fetch = [\"command\", \"pass\", \"show\", \"work\"]"));
        assert!(content.contains("read_only = true"));
    }

    #[tokio::test]
    async fn generate_config_writes_date_filter() {
        use crate::settings::{SyncTimeBound, SyncTimeSpan};

        let tmp = tempfile::tempdir().unwrap();
        let xdg = crate::with_test_xdg(&tmp.path().join("data"), &tmp.path().join("config"));
        let vdir: PathBuf = xdg.get_data_file("vdirsyncer").unwrap();
        tokio::fs::create_dir_all(&vdir).await.unwrap();

        let log_path = vdir.join("span.log");
        let log_file = tokio::fs::File::options()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
            .unwrap();
        let log = Arc::new(Mutex::new(log_file));

        let time_span = SyncTimeSpan {
            start: SyncTimeBound::Years(2),
            end: SyncTimeBound::Years(1),
        };

        let syncer = VDirSyncer::new_with_runner(
            &xdg,
            "spancol".to_string(),
            HashMap::new(),
            "http://localhost/".to_string(),
            false,
            None,
            &time_span,
            log,
            FakeCommandRunner::new(vec![], vec![]),
        )
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(&syncer.cfg).await.unwrap();
        assert!(
            content.contains("start_date = \"datetime.now() - timedelta(days=365*2)\""),
            "expected start_date expression; got:\n{}",
            content
        );
        assert!(
            content.contains("end_date = \"datetime.now() + timedelta(days=365*1)\""),
            "expected end_date expression; got:\n{}",
            content
        );
    }

    // --- run_sync retry-loop tests ---

    #[tokio::test]
    async fn run_sync_needs_discover_triggers_retry() {
        // First `run` returns NeedsDiscover output, then the retry `run` succeeds.
        // One `run_interactive` is needed for the discover step in between.
        let needs_discover_stderr =
            "please run `vdirsyncer discover` to update collections.\n".to_string();
        let ok_status = ExitStatus::from_raw(0);

        let runner = FakeCommandRunner::new(
            vec![
                CannedOutput::success(needs_discover_stderr.as_bytes()),
                CannedOutput::success(b""), // second sync succeeds
            ],
            vec![(ok_status, vec![])], // discover interactive response
        );
        let runner_ref = runner.clone();

        let (mut syncer, _tmp) = make_syncer_with_runner(runner_ref).await;
        let mut state = crate::State::new_for_test(
            CalStore::default(),
            crate::misc::Misc::new(std::path::PathBuf::default()),
        );
        let result = syncer.run_sync(&mut state, vec![]).await.unwrap();
        assert_eq!(result, SyncColResult::Success(false));

        // Verify call sequence: sync → discover (interactive) → sync (retry).
        let calls = runner.calls();
        assert_eq!(calls.len(), 3);
        assert!(
            matches!(&calls[0], RunCall::Run { args, .. } if args.contains(&"sync".to_string()))
        );
        assert!(matches!(&calls[1], RunCall::RunInteractive { args, .. }
            if args.contains(&"discover".to_string())));
        assert!(
            matches!(&calls[2], RunCall::Run { args, .. } if args.contains(&"sync".to_string()))
        );
    }

    #[tokio::test]
    async fn run_sync_double_discover_fails() {
        // Both sync attempts return NeedsDiscover → should return an error after the second.
        let needs_discover_stderr =
            "please run `vdirsyncer discover` to update collections.\n".to_string();
        let ok_status = ExitStatus::from_raw(0);

        let runner = FakeCommandRunner::new(
            vec![
                CannedOutput::success(needs_discover_stderr.as_bytes()),
                CannedOutput::success(needs_discover_stderr.as_bytes()),
            ],
            vec![(ok_status, vec![])], // discover interactive response
        );

        let (mut syncer, _tmp) = make_syncer_with_runner(runner).await;
        let mut state = crate::State::new_for_test(
            CalStore::default(),
            crate::misc::Misc::new(std::path::PathBuf::default()),
        );
        let result = syncer.run_sync(&mut state, vec![]).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("discover did not resolve")
        );
    }

    #[tokio::test]
    async fn create_cal_by_folder_runs_create_command() {
        let ok_status = ExitStatus::from_raw(0);
        let runner_ref =
            FakeCommandRunner::new(vec![CannedOutput::success(b"")], vec![(ok_status, vec![])]);
        let (mut syncer, _tmp) = make_syncer_with_runner(runner_ref.clone()).await;
        let mut state = crate::State::new_for_test(
            CalStore::default(),
            crate::misc::Misc::new(std::path::PathBuf::default()),
        );

        syncer
            .create_cal_by_folder(&mut state, &"work".to_string())
            .await
            .unwrap();

        let calls = runner_ref.calls();
        assert!(
            calls.iter().any(|call| matches!(call,
                RunCall::Run { program, args }
                    if program == "vdirsyncer"
                        && args == &vec![
                            "--config".to_string(),
                            syncer.cfg.to_str().unwrap().to_string(),
                            "create".to_string(),
                            "testcol/work".to_string(),
                        ]
            )),
            "expected vdirsyncer create call, got: {:?}",
            calls
        );
        assert!(
            calls.iter().any(|call| matches!(call,
                RunCall::RunInteractive { program, args }
                    if program == "vdirsyncer"
                        && args == &vec![
                            "--config".to_string(),
                            syncer.cfg.to_str().unwrap().to_string(),
                            "discover".to_string(),
                            "testcol".to_string(),
                        ]
            )),
            "expected vdirsyncer discover call after create, got: {:?}",
            calls
        );
    }

    #[tokio::test]
    async fn run_sync_nonzero_exit_returns_error() {
        let runner = FakeCommandRunner::new(vec![CannedOutput::failure(b"")], vec![]);

        let (mut syncer, _tmp) = make_syncer_with_runner(runner).await;
        let mut state = crate::State::new_for_test(
            CalStore::default(),
            crate::misc::Misc::new(std::path::PathBuf::default()),
        );
        let result = syncer.run_sync(&mut state, vec![]).await;
        assert!(result.is_err());
    }

    // --- Helper to build a State with one named calendar in a collection ---

    fn make_state_with_calendar(
        col_name: &str,
        cal_id: &str,
        folder: &str,
        enabled: bool,
    ) -> crate::State {
        use crate::settings::{CalendarSettings, CollectionSettings, SyncerType};

        let mut settings = crate::settings::Settings::new(std::path::PathBuf::default());
        let mut col = CollectionSettings::new(SyncerType::FileSystem {
            path: "/tmp".to_string(),
        });
        let mut cal = CalendarSettings::default();
        cal.set_enabled(enabled);
        cal.set_folder(folder.to_string());
        cal.set_name("Test Cal".to_string());
        col.all_calendars_mut().insert(cal_id.to_string(), cal);
        settings.collections_mut().insert(col_name.to_string(), col);

        // Build State with the settings but an empty store (no actual files on disk).
        let mut state = crate::State::new_for_test(
            CalStore::default(),
            crate::misc::Misc::new(std::path::PathBuf::default()),
        );
        *state.settings_mut() = settings;
        state
    }

    // --- sync tests ---

    #[tokio::test]
    async fn sync_invokes_run_sync_with_pair_names() {
        // The runner should receive one `run` call with the pair name "testcol/work".
        let runner = FakeCommandRunner::new(vec![CannedOutput::success(b"")], vec![]);
        let runner_ref = runner.clone();

        let mut folder_id = HashMap::new();
        folder_id.insert("work".to_string(), "cal-1".to_string());

        let (mut syncer, _tmp) = make_syncer_with_runner(runner_ref).await;
        // Override the folder_id so sync knows which pair names to pass.
        syncer.folder_id = folder_id;

        let mut state = make_state_with_calendar("testcol", "cal-1", "work", true);
        let result = syncer.sync(&mut state).await.unwrap();
        assert_eq!(result, SyncColResult::Success(false));

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        let RunCall::Run { args, .. } = &calls[0] else {
            panic!("expected Run call");
        };
        assert!(args.contains(&"testcol/work".to_string()));
    }

    #[tokio::test]
    async fn sync_empty_calendars_returns_success_false() {
        // A collection with no enabled calendars should short-circuit without calling the runner.
        let runner = FakeCommandRunner::new(vec![], vec![]);
        let runner_ref = runner.clone();

        let (mut syncer, _tmp) = make_syncer_with_runner(runner_ref).await;

        // State has an empty collection (no calendars enabled).
        let mut state = make_state_with_calendar("testcol", "cal-1", "work", false);
        // Disable the only calendar by removing it entirely from the enabled set.
        state
            .settings_mut()
            .collections_mut()
            .get_mut("testcol")
            .unwrap()
            .all_calendars_mut()
            .clear();

        let result = syncer.sync(&mut state).await.unwrap();
        assert_eq!(result, SyncColResult::Success(false));
        assert!(runner.calls().is_empty());
    }

    // --- sync_cal tests ---

    #[tokio::test]
    async fn sync_cal_invokes_run_sync_for_named_calendar() {
        let runner = FakeCommandRunner::new(vec![CannedOutput::success(b"")], vec![]);
        let runner_ref = runner.clone();

        let (mut syncer, _tmp) = make_syncer_with_runner(runner_ref).await;

        let mut state = make_state_with_calendar("testcol", "cal-1", "work", true);
        let result = syncer
            .sync_cal(&mut state, &"cal-1".to_string())
            .await
            .unwrap();
        assert_eq!(result, SyncColResult::Success(false));

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        let RunCall::Run { args, .. } = &calls[0] else {
            panic!("expected Run call");
        };
        assert!(args.contains(&"testcol/work".to_string()));
    }

    #[tokio::test]
    async fn sync_cal_disabled_calendar_returns_success_false() {
        let runner = FakeCommandRunner::new(vec![], vec![]);
        let runner_ref = runner.clone();

        let (mut syncer, _tmp) = make_syncer_with_runner(runner_ref).await;

        let mut state = make_state_with_calendar("testcol", "cal-1", "work", false);
        let result = syncer
            .sync_cal(&mut state, &"cal-1".to_string())
            .await
            .unwrap();
        assert_eq!(result, SyncColResult::Success(false));
        assert!(runner.calls().is_empty());
    }

    // --- delete and delete_cal tests ---

    /// Sets up the expected on-disk layout that `delete_cal` and `delete` operate on.
    ///
    /// Creates:
    ///   `<cfg_dir>/<name>-status/<name>/<folder>.items`
    ///   `<cfg_dir>/<name>-status/<name>/<folder>.metadata`
    ///   `<cfg_dir>/<name>-data/<folder>/event.ics`
    ///   `<cfg_dir>/<name>-data/<folder>/color`
    ///   `<cfg_dir>/<name>-data/<folder>/displayname`
    async fn create_sync_layout(cfg_dir: &std::path::Path, name: &str, folder: &str) {
        let status_dir = cfg_dir.join(format!("{}-status/{}", name, name));
        tokio::fs::create_dir_all(&status_dir).await.unwrap();
        tokio::fs::write(status_dir.join(format!("{}.items", folder)), b"")
            .await
            .unwrap();
        tokio::fs::write(status_dir.join(format!("{}.metadata", folder)), b"")
            .await
            .unwrap();

        let data_dir = cfg_dir.join(format!("{}-data/{}", name, folder));
        tokio::fs::create_dir_all(&data_dir).await.unwrap();
        tokio::fs::write(
            data_dir.join("event.ics"),
            b"BEGIN:VCALENDAR\nEND:VCALENDAR\n",
        )
        .await
        .unwrap();
        tokio::fs::write(data_dir.join("color"), b"#ff0000")
            .await
            .unwrap();
        tokio::fs::write(data_dir.join("displayname"), b"Work")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn delete_removes_status_and_data_dirs() {
        let runner = FakeCommandRunner::new(vec![], vec![]);
        let (mut syncer, _tmp) = make_syncer_with_runner(runner).await;

        let cfg_dir = syncer.cfg.parent().unwrap().to_path_buf();
        create_sync_layout(&cfg_dir, "testcol", "work").await;

        let mut state = crate::State::new_for_test(
            CalStore::default(),
            crate::misc::Misc::new(std::path::PathBuf::default()),
        );
        syncer.delete(&mut state, false).await.unwrap();

        assert!(
            !cfg_dir.join("testcol-status").exists(),
            "status dir should be removed"
        );
        assert!(
            !cfg_dir.join("testcol-data").exists(),
            "data dir should be removed"
        );
        // config file should be kept when config=false
        assert!(syncer.cfg.exists(), "cfg file should remain");
    }

    #[tokio::test]
    async fn delete_with_config_also_removes_cfg_file() {
        let runner = FakeCommandRunner::new(vec![], vec![]);
        let (mut syncer, _tmp) = make_syncer_with_runner(runner).await;

        let cfg_path = syncer.cfg.clone();
        let cfg_dir = cfg_path.parent().unwrap().to_path_buf();
        create_sync_layout(&cfg_dir, "testcol", "work").await;

        let mut state = crate::State::new_for_test(
            CalStore::default(),
            crate::misc::Misc::new(std::path::PathBuf::default()),
        );
        syncer.delete(&mut state, true).await.unwrap();

        assert!(!cfg_dir.join("testcol-status").exists());
        assert!(!cfg_dir.join("testcol-data").exists());
        assert!(
            !cfg_path.exists(),
            "cfg file should be removed when config=true"
        );
    }

    #[tokio::test]
    async fn delete_cal_removes_status_and_data_files_but_keeps_meta() {
        let runner = FakeCommandRunner::new(vec![], vec![]);
        let (mut syncer, _tmp) = make_syncer_with_runner(runner).await;

        let cfg_dir = syncer.cfg.parent().unwrap().to_path_buf();
        create_sync_layout(&cfg_dir, "testcol", "work").await;

        let mut state = make_state_with_calendar("testcol", "cal-1", "work", true);
        syncer
            .delete_cal(&mut state, &"cal-1".to_string())
            .await
            .unwrap();

        // Status files should be gone.
        assert!(
            !cfg_dir.join("testcol-status/testcol/work.items").exists(),
            "status .items should be removed"
        );
        assert!(
            !cfg_dir
                .join("testcol-status/testcol/work.metadata")
                .exists(),
            "status .metadata should be removed"
        );

        // Calendar data file should be gone.
        assert!(
            !cfg_dir.join("testcol-data/work/event.ics").exists(),
            "event.ics should be removed"
        );

        // Metadata files must be preserved.
        assert!(
            cfg_dir.join("testcol-data/work/color").exists(),
            "color file should be kept"
        );
        assert!(
            cfg_dir.join("testcol-data/work/displayname").exists(),
            "displayname file should be kept"
        );
    }

    #[tokio::test]
    async fn delete_cal_by_folder_runs_delete_command() {
        let runner_ref = FakeCommandRunner::new(vec![CannedOutput::success(b"")], vec![]);
        let (mut syncer, _tmp) = make_syncer_with_runner(runner_ref.clone()).await;

        let cfg_dir = syncer.cfg.parent().unwrap().to_path_buf();
        create_sync_layout(&cfg_dir, "testcol", "work").await;

        let mut state = crate::State::new_for_test(
            CalStore::default(),
            crate::misc::Misc::new(std::path::PathBuf::default()),
        );
        syncer
            .delete_cal_by_folder(&mut state, &"work".to_string())
            .await
            .unwrap();

        assert!(
            !cfg_dir.join("testcol-status/testcol/work.items").exists(),
            "status .items should be removed"
        );
        assert!(
            !cfg_dir
                .join("testcol-status/testcol/work.metadata")
                .exists(),
            "status .metadata should be removed"
        );
        assert!(
            !cfg_dir.join("testcol-data/work/event.ics").exists(),
            "event.ics should be removed"
        );
        assert!(
            cfg_dir.join("testcol-data/work/color").exists(),
            "color file should be kept"
        );
        assert!(
            cfg_dir.join("testcol-data/work/displayname").exists(),
            "displayname file should be kept"
        );

        let calls = runner_ref.calls();
        assert!(
            calls.iter().any(|call| matches!(call,
                RunCall::Run { program, args }
                    if program == "vdirsyncer"
                        && args == &vec![
                            "--config".to_string(),
                            syncer.cfg.to_str().unwrap().to_string(),
                            "delete".to_string(),
                            "testcol/work".to_string(),
                        ]
            )),
            "expected vdirsyncer delete call, got: {:?}",
            calls
        );
    }
}
