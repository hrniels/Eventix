// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::Context;
use axum::http::StatusCode;
use eventix_ical::objects::EventLike;
use eventix_state::{CollectionSettings, EmailAccount, EventixState, SyncerType};
use serde_json::Value;
use tempfile::TempDir;

use crate::helper::{
    COL_ID, encode_form, make_calendars_api_router, make_router, post, post_query,
};

pub const USERNAME: &str = "test";
pub const PASSWORD: &str = "testpass";
pub const REMOTE_CALENDAR_FOLDER: &str = "work";
pub const REMOTE_CALENDAR_NAME: &str = "Work";
pub const REMOTE_CALENDAR2_FOLDER: &str = "personal";
pub const REMOTE_CALENDAR2_NAME: &str = "Personal";

pub fn binaries_available() -> bool {
    find_binary("radicale").is_some()
}

fn free_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn find_binary(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

pub struct RadicaleServer {
    _tmp: TempDir,
    port: u16,
    url: String,
    child: Child,
}

impl RadicaleServer {
    pub fn start() -> anyhow::Result<Self> {
        let radicale =
            find_binary("radicale").ok_or_else(|| anyhow::anyhow!("radicale not found"))?;
        let tmp = TempDir::new()?;
        let port = free_port()?;
        let storage_dir = tmp.path().join("storage");
        fs::create_dir_all(&storage_dir)?;

        let users_path = tmp.path().join("users");
        fs::write(&users_path, format!("{USERNAME}:{PASSWORD}\n"))?;

        // Keep the server fully self-contained inside the tempdir so tests never depend on a
        // long-running system-wide Radicale instance.
        let config_path = tmp.path().join("config");
        Self::write_config(&config_path, port, &users_path, &storage_dir)?;

        let stdout = fs::File::create(tmp.path().join("radicale.stdout.log"))?;
        let stderr = fs::File::create(tmp.path().join("radicale.stderr.log"))?;
        let child = Command::new(radicale)
            .arg("--config")
            .arg("")
            .arg(&config_path)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()?;

        let url = format!("http://127.0.0.1:{port}/{USERNAME}/");
        let mut server = Self {
            _tmp: tmp,
            port,
            url,
            child,
        };
        server.wait_until_ready()?;
        Ok(server)
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    fn write_config(
        path: &Path,
        port: u16,
        users_path: &Path,
        storage_dir: &Path,
    ) -> anyhow::Result<()> {
        fs::write(
            path,
            format!(
                "[server]\n\
                 hosts = 127.0.0.1:{port}\n\n\
                 [auth]\n\
                 type = htpasswd\n\
                 htpasswd_filename = {}\n\
                 htpasswd_encryption = plain\n\
                 delay = 0\n\n\
                 [rights]\n\
                 type = owner_only\n\n\
                 [storage]\n\
                 filesystem_folder = {}\n\n\
                 [logging]\n\
                 level = warning\n",
                users_path.display(),
                storage_dir.display()
            ),
        )
        .context("create radicale config")
    }

    fn wait_until_ready(&mut self) -> anyhow::Result<()> {
        let addr = ([127, 0, 0, 1], self.port);
        for _ in 0..50 {
            if let Some(status) = self.child.try_wait()? {
                return Err(anyhow::anyhow!("radicale exited early with {status}"));
            }
            if TcpStream::connect_timeout(&addr.into(), Duration::from_millis(200)).is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        Err(anyhow::anyhow!("radicale did not become ready in time"))
    }
}

impl Drop for RadicaleServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn password_cmd() -> Option<Vec<String>> {
    Some(vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("printf %s {PASSWORD}"),
    ])
}

pub struct RadicalePeer {
    state: EventixState,
    tmp: TempDir,
}

impl RadicalePeer {
    fn new(state: EventixState, tmp: TempDir) -> Self {
        Self { state, tmp }
    }

    pub async fn create_calendar(&self, name: &str, expected_folder: &str) -> String {
        assert!(
            self.calendar_id_for_folder(expected_folder).await.is_none(),
            "to-be-created calendar does exist?"
        );

        // Drive remote calendar creation through Eventix itself so the tests exercise the same API
        // path users would hit.
        let (status, body) = post_query(
            make_calendars_api_router(self.state.clone()),
            &format!("/api/calendars/addcal?col_id={COL_ID}&name={name}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");

        self.calendar_id_for_folder(expected_folder)
            .await
            .expect("created calendar does not exist?")
    }

    pub async fn create_todo(&self, cal_id: &str, summary: &str) -> anyhow::Result<()> {
        let body = encode_form(&[
            ("quicktodo_calendar", cal_id),
            ("summary", summary),
            ("due_date", "2026-04-20"),
        ]);
        let (status, resp) = post(make_router(self.state.clone()), "/api/items/add", &body).await;
        if status != StatusCode::OK {
            return Err(anyhow::anyhow!("add failed with {status}: {resp}"));
        }
        Ok(())
    }

    pub async fn create_and_sync_todo(&self, cal_id: &str, summary: &str) -> anyhow::Result<()> {
        self.create_todo(cal_id, summary).await?;
        self.sync_collection().await;
        Ok(())
    }

    pub async fn discover_collection(&self) -> Value {
        self.syncop_json(&format!(
            "/api/calendars/syncop?op[type]=DiscoverCollection&op[data][col_id]={COL_ID}"
        ))
        .await
    }

    pub async fn sync_collection(&self) -> Value {
        self.syncop_json(&format!(
            "/api/calendars/syncop?op[type]=SyncCollection&op[data][col_id]={COL_ID}"
        ))
        .await
    }

    pub async fn sync_all(&self) -> Value {
        self.syncop_json("/api/calendars/syncop?op[type]=SyncAll")
            .await
    }

    pub async fn reload_collection(&self) -> Value {
        self.syncop_json(&format!(
            "/api/calendars/syncop?op[type]=ReloadCollection&op[data][col_id]={COL_ID}"
        ))
        .await
    }

    pub async fn reload_calendar(&self, cal_id: &str) -> Value {
        self.syncop_json(&format!(
            "/api/calendars/syncop?op[type]=ReloadCalendar&op[data][col_id]={COL_ID}&op[data][cal_id]={cal_id}"
        ))
        .await
    }

    pub async fn uid_for_summary(&self, summary: &str) -> String {
        let locked = self.state.lock().await;
        locked
            .store()
            .files()
            .find_map(|file| {
                file.calendar().components().iter().find_map(|comp| {
                    (comp.rid().is_none() && comp.summary().is_some_and(|s| s == summary))
                        .then(|| comp.uid().clone())
                })
            })
            .unwrap_or_else(|| panic!("no store entry found for summary '{summary}'"))
    }

    pub async fn assert_store_summary(&self, uid: &str, expected: &str) {
        let locked = self.state.lock().await;
        let file = locked
            .store()
            .file_by_id(uid)
            .expect("synced file in store");
        let component = file
            .calendar()
            .components()
            .iter()
            .find(|comp| comp.rid().is_none())
            .expect("base component present");
        assert_eq!(component.summary().map(String::as_str), Some(expected));
    }

    pub fn calendar_dir(&self, folder: &str) -> PathBuf {
        self.tmp
            .path()
            .join("data")
            .join("vdirsyncer")
            .join(format!("{COL_ID}-data"))
            .join(folder)
    }

    pub fn overwrite_cached_todo(&self, folder: &str, uid: &str, summary: &str) {
        // Reload tests intentionally corrupt the local cache to verify that the remote source wins
        // again after reload.
        fs::write(
            self.calendar_dir(folder).join(format!("{uid}.ics")),
            format!(
                "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:{uid}\r\nDTSTAMP:20260101T000000Z\r\nDUE;VALUE=DATE:20260420\r\nSUMMARY:{summary}\r\nEND:VTODO\r\nEND:VCALENDAR\r\n"
            ),
        )
        .expect("rewrite local cached event");
    }

    async fn calendar_id_for_folder(&self, folder: &str) -> Option<String> {
        let locked = self.state.lock().await;
        locked
            .settings()
            .collections()
            .get(COL_ID)
            .and_then(|collection| {
                collection
                    .all_calendars()
                    .iter()
                    .find_map(|(id, calendar)| (calendar.folder() == folder).then(|| id.clone()))
            })
    }

    async fn syncop_json(&self, uri: &str) -> Value {
        let (status, body) = post_query(make_calendars_api_router(self.state.clone()), uri).await;
        assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");
        serde_json::from_str(&body).expect("parse JSON response")
    }
}

pub struct RadicalePair {
    _server: RadicaleServer,
    consumer: RadicalePeer,
    producer: RadicalePeer,
}

impl RadicalePair {
    pub async fn new() -> Self {
        let server = RadicaleServer::start().expect("start Radicale test server");
        // Start two isolated Eventix peers against the same Radicale backend so tests can model
        // one side producing remote changes and the other side consuming them.
        let (consumer_state, consumer_tmp) =
            crate::helper::make_state_from_col(Self::make_empty_collection(server.url()));
        let (producer_state, producer_tmp) =
            crate::helper::make_state_from_col(Self::make_empty_collection(server.url()));

        Self {
            _server: server,
            consumer: RadicalePeer::new(consumer_state, consumer_tmp),
            producer: RadicalePeer::new(producer_state, producer_tmp),
        }
    }

    fn make_empty_collection(server_url: &str) -> CollectionSettings {
        CollectionSettings::new(SyncerType::VDirSyncer {
            email: EmailAccount::new("Test User".to_string(), "test@example.com".to_string()),
            url: server_url.to_string(),
            read_only: false,
            username: Some(USERNAME.to_string()),
            password_cmd: password_cmd(),
            time_span: Default::default(),
        })
    }

    pub fn consumer(&self) -> &RadicalePeer {
        &self.consumer
    }

    pub fn producer(&self) -> &RadicalePeer {
        &self.producer
    }
}
