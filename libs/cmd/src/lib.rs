use anyhow::{Context, anyhow};
use chrono::Local;
use eventix_ical::{col::CalFile, objects::EventLike};
use eventix_state::EventixState;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{fs::File, path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    task,
};
use tracing::{debug, error};
use xdg::BaseDirectories;

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Import(ImportOptions),
    TaskStatus,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Response {
    Success,
    TaskStatus(u32, u32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportOptions {
    pub file: String,
    pub calendar: String,
}

async fn acquire_lock(xdg: &BaseDirectories) -> anyhow::Result<File> {
    let path = xdg
        .get_runtime_directory()
        .context("get path to runtime directory")?;
    let path = path.join("eventix.lock");
    task::spawn_blocking(|| {
        let f = File::create(path).context("create eventix.lock")?;
        f.lock_exclusive().context("acquire eventix.lock")?;
        Ok(f)
    })
    .await
    .unwrap()
}

pub async fn handle_commands(xdg: &BaseDirectories, state: EventixState) -> anyhow::Result<()> {
    let socket_path = get_socket_path(xdg);

    // remove it in case it already exists; that's okay because we only get here if the server
    // wasn't running yet.
    std::fs::remove_file(&socket_path).ok();

    let listener = UnixListener::bind(&socket_path)?;
    debug!("cmds: listening on {:?}", socket_path);

    loop {
        match listener.accept().await {
            Ok((mut stream, _addr)) => {
                if let Err(e) = parse_and_handle(state.clone(), &mut stream).await {
                    error!("command failed: {}", e);
                }
            }
            Err(e) => error!("accept failed: {}", e),
        }
    }
}

async fn marshall_msg<T>(stream: &mut UnixStream, data: T) -> anyhow::Result<()>
where
    T: Serialize,
{
    let msg = serde_json::to_string(&data)?;
    stream.write_all(&(msg.len() as u32).to_be_bytes()).await?;
    stream.write_all(msg.as_bytes()).await?;
    Ok(())
}

async fn unmarshall_msg<T>(stream: &mut UnixStream) -> anyhow::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let s = String::from_utf8(buf)?;
    let msg: T = serde_json::from_str(&s)?;
    Ok(msg)
}

async fn parse_and_handle(state: EventixState, stream: &mut UnixStream) -> anyhow::Result<()> {
    let req: Request = unmarshall_msg(stream).await?;
    let resp = handle_request(state, req).await?;
    marshall_msg(stream, resp).await?;
    Ok(())
}

pub async fn send_or_execute(
    xdg: &BaseDirectories,
    state: EventixState,
    req: Request,
) -> anyhow::Result<Response> {
    let path = get_socket_path(xdg);
    if let Ok(stream) = UnixStream::connect(&path).await {
        do_send(xdg, req, stream).await
    } else {
        handle_request(state, req).await
    }
}

pub async fn send(xdg: &BaseDirectories, req: Request) -> anyhow::Result<Response> {
    let path = get_socket_path(xdg);
    let stream = UnixStream::connect(&path).await?;
    do_send(xdg, req, stream).await
}

async fn do_send(
    xdg: &BaseDirectories,
    req: Request,
    mut stream: UnixStream,
) -> anyhow::Result<Response> {
    // ensure that not two processes use this socket at the same time
    let _lockfile = acquire_lock(xdg).await?;

    marshall_msg(&mut stream, req).await?;
    let resp: Response = unmarshall_msg(&mut stream).await?;

    Ok(resp)
}

async fn handle_request(state: EventixState, req: Request) -> anyhow::Result<Response> {
    match req {
        Request::Import(req) => handle_import(state, req).await,
        Request::TaskStatus => handle_task_status(state).await,
    }
}

async fn handle_import(state: EventixState, req: ImportOptions) -> anyhow::Result<Response> {
    let save_all = |files: &Vec<CalFile>| {
        for f in files {
            f.save()?;
        }
        Ok(())
    };

    let mut state = state.lock().await;
    let cal = Arc::from(req.calendar.clone());
    let dir = state
        .store_mut()
        .directory_mut(&cal)
        .ok_or_else(|| anyhow!("Unknown calendar '{}'", req.calendar))?;

    let files =
        CalFile::new_from_external_file(cal.clone(), dir.path().clone(), req.file.clone().into())
            .context(format!("Parsing file '{}' failed", req.file))?;

    // first delete any existing files with those uids
    for f in &files {
        let uid = f.components().first().unwrap().uid();
        // TODO note that we cannot undo this step
        dir.delete_by_uid(uid).ok();
    }

    // now try to save all and undo these saves, if an error occurs
    if let Err(e) = save_all(&files) {
        for mut f in files {
            f.remove().ok();
        }
        return Err(e);
    }

    // all good; add them to the directory
    for f in files {
        dir.add_file(f);
    }

    Ok(Response::Success)
}

async fn handle_task_status(state: EventixState) -> anyhow::Result<Response> {
    let state = state.lock().await;
    let tz = *state.settings().locale().timezone();
    let today = Local::now().date_naive();

    let due_today = eventix_state::util::due_todos(&state, &tz, 1)
        .filter(|o| o.occurrence_ends_on(today))
        .count();

    let overdue = eventix_state::util::overdue_todos(&state, &tz).count();

    Ok(Response::TaskStatus(due_today as u32, overdue as u32))
}

fn get_socket_path(xdg: &BaseDirectories) -> PathBuf {
    let path = xdg
        .get_runtime_directory()
        .cloned()
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    path.join("eventix.sock")
}
