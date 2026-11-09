// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use eventix_ical::objects::{CalCompType, CalDate, CalDateType, EventLike};
use eventix_state::{
    CollectionSettings, EmailAccount, EventixState, Settings, State, SyncCancellation, SyncResult,
    SyncerType,
    actions::{
        Action,
        calendar::{
            CreateCalendar, DeleteCalendarByFolder, DiscoverCollection, ReloadCalendar,
            ReloadCollection, SyncAll, SyncCollection,
        },
        items::CreateItem,
    },
    encrypt_password, retrieve_portal_secret,
};
use tempfile::TempDir;

pub const COL_ID: &str = "col1";
// Match eventix-getpw, which vdirsyncer invokes to read this peer's credentials.
const APP_ID: &str = if cfg!(debug_assertions) {
    "io.github.hrniels.Eventix-debug"
} else {
    "io.github.hrniels.Eventix"
};

pub const USERNAME: &str = "test";
pub const PASSWORD: &str = "testpass";
pub const REMOTE_CALENDAR_FOLDER: &str = "work";
pub const REMOTE_CALENDAR_NAME: &str = "Work";
pub const REMOTE_CALENDAR2_FOLDER: &str = "personal";
pub const REMOTE_CALENDAR2_NAME: &str = "Personal";

const EXPECTED_VDIRSYNCER_VERSION: &str = "0.20.0+eventix";

fn expected_vdirsyncer_available() -> bool {
    let Some(vdirsyncer) = find_binary("vdirsyncer") else {
        return false;
    };
    let Ok(output) = Command::new(vdirsyncer).arg("--version").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout
        .trim()
        .strip_prefix("vdirsyncer, version ")
        .unwrap_or(stdout.trim());
    version == EXPECTED_VDIRSYNCER_VERSION
}

#[allow(clippy::assertions_on_constants)]
pub fn check_requirements() -> Option<String> {
    if find_binary("radicale").is_none() {
        return Some("skipping radicale test as radicale is missing".to_string());
    }
    if !expected_vdirsyncer_available() {
        assert!(
            false,
            "Cannot run radicale tests as the Eventix vdirsyncer is not active.\nPlease run tests via ./b test."
        );
        return Some("skipping radicale test".to_string());
    }
    None
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

pub struct RadicalePeer {
    state: EventixState,
    tmp: TempDir,
}

impl RadicalePeer {
    pub fn state(&self) -> &EventixState {
        &self.state
    }

    fn new(state: EventixState, tmp: TempDir) -> Self {
        Self { state, tmp }
    }

    pub async fn create_calendar(&self, name: &str, expected_folder: &str) -> String {
        assert!(
            self.calendar_id_for_folder(expected_folder).await.is_none(),
            "to-be-created calendar does exist?"
        );

        self.try_create_calendar(name)
            .await
            .expect("create calendar");

        let cal_id = self
            .calendar_id_for_folder(expected_folder)
            .await
            .expect("created calendar does not exist?");

        // New calendars have no configured item types until the user selects them.
        let mut state = self.state.lock().await;
        state
            .settings_mut()
            .collections_mut()
            .get_mut(COL_ID)
            .expect("test collection exists")
            .all_calendars_mut()
            .get_mut(&cal_id)
            .expect("created calendar exists")
            .set_types(vec![CalCompType::Event, CalCompType::Todo]);
        state
            .settings()
            .write_to_file()
            .expect("save calendar types");

        cal_id
    }

    pub async fn try_create_calendar(&self, name: &str) -> anyhow::Result<()> {
        Box::new(CreateCalendar {
            id: COL_ID.into(),
            name: name.into(),
        })
        .execute(&self.state)
        .await
    }

    pub async fn delete_calendar_by_folder(&self, folder: &str) {
        self.try_delete_calendar_by_folder(folder)
            .await
            .expect("delete calendar");
    }

    pub async fn try_delete_calendar_by_folder(&self, folder: &str) -> anyhow::Result<()> {
        Box::new(DeleteCalendarByFolder {
            col_id: COL_ID.into(),
            folder: folder.into(),
        })
        .execute(&self.state)
        .await
    }

    pub async fn create_todo(&self, cal_id: &str, summary: &str) -> anyhow::Result<()> {
        Box::new(CreateItem {
            kind: CalCompType::Todo,
            task_status: None,
            calendar: cal_id.into(),
            summary: summary.into(),
            location: String::new(),
            description: String::new(),
            start: None,
            end: Some(CalDate::Date("2026-04-20".parse()?, CalDateType::Inclusive)),
            recurrence: None,
            alarms: vec![],
            personal_alarms: None,
            attendees: vec![],
        })
        .execute(&self.state)
        .await
        .map(|_| ())
    }

    pub async fn create_and_sync_todo(&self, cal_id: &str, summary: &str) -> anyhow::Result<()> {
        self.create_todo(cal_id, summary).await?;
        self.sync_collection().await;
        Ok(())
    }

    pub async fn discover_collection(&self) -> SyncResult {
        Box::new(DiscoverCollection {
            col_id: COL_ID.into(),
            auth_url: None,
            cancellation: SyncCancellation::default(),
        })
        .execute(&self.state)
        .await
        .expect("discover collection")
    }

    pub async fn sync_collection(&self) -> SyncResult {
        Box::new(SyncCollection {
            col_id: COL_ID.into(),
            auth_url: None,
            cancellation: SyncCancellation::default(),
        })
        .execute(&self.state)
        .await
        .expect("sync collection")
    }

    pub async fn sync_all(&self) -> SyncResult {
        Box::new(SyncAll {
            auth_url: None,
            progress: None,
            cancellation: SyncCancellation::default(),
        })
        .execute(&self.state)
        .await
        .expect("sync all")
    }

    pub async fn reload_collection(&self) -> SyncResult {
        Box::new(ReloadCollection {
            col_id: COL_ID.into(),
            auth_url: None,
            cancellation: SyncCancellation::default(),
        })
        .execute(&self.state)
        .await
        .expect("reload collection")
    }

    pub async fn reload_calendar(&self, cal_id: &str) -> SyncResult {
        Box::new(ReloadCalendar {
            col_id: COL_ID.into(),
            cal_id: cal_id.into(),
            auth_url: None,
            cancellation: SyncCancellation::default(),
        })
        .execute(&self.state)
        .await
        .expect("reload calendar")
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

    pub async fn assert_store_missing(&self, uid: &str) {
        let locked = self.state.lock().await;
        assert!(
            locked.store().file_by_id(uid).is_none(),
            "unexpected store entry for uid '{uid}'"
        );
    }

    pub fn calendar_dir(&self, folder: &str) -> PathBuf {
        self.tmp
            .path()
            .join("data")
            .join(APP_ID)
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
}

pub struct RadicalePair {
    _server: RadicaleServer,
    consumer: RadicalePeer,
    producer: RadicalePeer,
}

impl RadicalePair {
    pub async fn new(consumer_read_only: bool, producer_read_only: bool) -> Self {
        let server = RadicaleServer::start().expect("start Radicale test server");
        // Start two isolated Eventix peers against the same Radicale backend so tests can model
        // one side producing remote changes and the other side consuming them.
        let (consumer_state, consumer_tmp) =
            make_state_from_col(Self::make_collection(server.url(), consumer_read_only).await);
        let (producer_state, producer_tmp) =
            make_state_from_col(Self::make_collection(server.url(), producer_read_only).await);

        Self {
            _server: server,
            consumer: RadicalePeer::new(consumer_state, consumer_tmp),
            producer: RadicalePeer::new(producer_state, producer_tmp),
        }
    }

    async fn make_collection(server_url: &str, read_only: bool) -> CollectionSettings {
        let secret = retrieve_portal_secret().await.unwrap();
        let pw = encrypt_password(&secret, PASSWORD).unwrap();

        CollectionSettings::new(SyncerType::VDirSyncer {
            email: EmailAccount::new("Test User".to_string(), "test@example.com".to_string()),
            url: server_url.to_string(),
            read_only,
            username: Some(USERNAME.to_string()),
            password: Some(pw),
            time_span: Default::default(),
        })
    }

    pub fn consumer(&self) -> &RadicalePeer {
        &self.consumer
    }

    pub fn producer(&self) -> &RadicalePeer {
        &self.producer
    }

    pub async fn spawn_peer(&self) -> RadicalePeer {
        let (state, tmp) =
            make_state_from_col(Self::make_collection(self._server.url(), false).await);
        RadicalePeer::new(state, tmp)
    }
}

fn make_state_from_col(collection: CollectionSettings) -> (EventixState, TempDir) {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    let config = tmp.path().join("config");
    let xdg = Arc::new(eventix_state::with_test_xdg_prefix(&data, &config, APP_ID));
    let mut settings = Settings::new(xdg.place_config_file("settings.toml").unwrap());
    settings.collections_mut().insert(COL_ID.into(), collection);
    settings.write_to_file().unwrap();
    let state = State::new(xdg).unwrap();
    (Arc::new(tokio::sync::Mutex::new(state)), tmp)
}
