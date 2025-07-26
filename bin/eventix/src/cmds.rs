use anyhow::{Context, anyhow};
use eventix_ical::{col::CalFile, objects::EventLike};
use eventix_state::EventixState;
use std::{env, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
use tracing::{debug, error};

use crate::{Command, ImportOptions};

pub async fn handle_commands(state: EventixState) -> anyhow::Result<()> {
    let socket_path = get_socket_path();

    // remove it in case it already exists; that's okay because we only get here if the server
    // wasn't running yet.
    std::fs::remove_file(&socket_path).ok();

    let listener = UnixListener::bind(&socket_path)?;
    debug!("cmds: listening on {:?}", socket_path);

    loop {
        match listener.accept().await {
            Ok((mut stream, _addr)) => {
                if let Err(e) = parse_command(state.clone(), &mut stream).await {
                    error!("command failed: {}", e);
                }
            }
            Err(e) => error!("accept failed: {}", e),
        }
    }
}

async fn parse_command(state: EventixState, stream: &mut UnixStream) -> anyhow::Result<()> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;

    let cmd: Command = serde_json::from_str(&String::from_utf8(buf)?)?;
    handle_command(state, cmd).await
}

pub async fn send_command(cmd: Command) -> anyhow::Result<()> {
    let mut stream = UnixStream::connect(&get_socket_path()).await?;
    let msg = serde_json::to_string(&cmd)?;
    stream.write_all(&(msg.len() as u32).to_be_bytes()).await?;
    stream.write_all(msg.as_bytes()).await?;
    Ok(())
}

pub async fn handle_command(state: EventixState, cmd: Command) -> anyhow::Result<()> {
    match cmd {
        Command::Import(cmd) => handle_import(state, cmd).await,
    }
}

async fn handle_import(state: EventixState, cmd: ImportOptions) -> anyhow::Result<()> {
    let save_all = |files: &Vec<CalFile>| {
        for f in files {
            f.save()?;
        }
        Ok(())
    };

    let mut state = state.lock().await;
    let cal = Arc::from(cmd.calendar.clone());
    let dir = state
        .store_mut()
        .directory_mut(&cal)
        .ok_or_else(|| anyhow!("Unknown calendar '{}'", cmd.calendar))?;

    let files =
        CalFile::new_from_external_file(cal.clone(), dir.path().clone(), cmd.file.clone().into())
            .context(format!("Parsing file '{}' failed", cmd.file))?;

    // first check if any UID already exists
    for f in &files {
        let uid = f.components().first().unwrap().uid();
        if dir.files().iter().any(|f| f.contains_uid(uid)) {
            return Err(anyhow!("UID '{}' does already exist", uid));
        }
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

    Ok(())
}

fn get_socket_path() -> String {
    env::var("XDG_RUNTIME_DIR")
        .map(|dir| format!("{dir}/eventix.sock"))
        .unwrap_or_else(|_| "/tmp/eventix.sock".to_string())
}
