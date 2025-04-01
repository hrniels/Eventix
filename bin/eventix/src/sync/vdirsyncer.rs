use anyhow::anyhow;
use async_trait::async_trait;
use std::process::Stdio;
use std::{process::Output, sync::Arc};
use tokio::process::Command;

use crate::state::EventixState;
use crate::sync::Syncer;

enum EventType<'a> {
    Add(&'a str),
    Update(&'a str, &'a str),
    Delete(&'a str, &'a str),
}

pub struct VDirSyncer {
    cmd: Command,
    local_name: String,
}

impl VDirSyncer {
    pub fn new(args: Vec<String>, local_name: String) -> Self {
        assert!(!args.is_empty());
        let mut cmd = Command::new(args[0].clone());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.args(&args[1..]);
        Self { cmd, local_name }
    }

    async fn post_process(
        &self,
        cal: &Arc<String>,
        state: EventixState,
        output: Output,
    ) -> anyhow::Result<bool> {
        let mut added = false;
        let mut changed = Vec::new();
        let mut deleted = Vec::new();
        for line in String::from_utf8(output.stderr)?.lines() {
            tracing::debug!("{}: {}", *cal, line);

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
                match ev {
                    // as the filename is not necessarily the UID and we only know the UID here, we
                    // do not collect them, but just remember that we found a new item.
                    EventType::Add(cal) if cal == self.local_name => added = true,
                    EventType::Update(uid, cal) if cal == self.local_name => {
                        changed.push(uid.to_string())
                    }
                    EventType::Delete(uid, cal) if cal == self.local_name => {
                        deleted.push(uid.to_string())
                    }
                    _ => {}
                }
            }
        }

        let seen_changes = added || !changed.is_empty() || !deleted.is_empty();

        let mut state = state.lock().await;
        let dir = state
            .store_mut()
            .directory_mut(cal)
            .ok_or_else(|| anyhow!("directory '{}' does not exist", cal))?;
        if added {
            // rescan the whole directory for new files as we only know the new UIDs, but not
            // necessarily their filenames (as these can be different).
            dir.rescan_for_additions()?;
        }
        for uid in changed {
            if let Some(file) = dir.file_by_id_mut(&uid) {
                file.reload_calendar()?;
            } else {
                tracing::warn!("file for uid {} does not exist", uid);
            }
        }
        for uid in deleted {
            dir.remove_by_uid(uid)?;
        }

        Ok(seen_changes)
    }
}

#[async_trait]
impl Syncer for VDirSyncer {
    async fn sync(&mut self, cal: &Arc<String>, state: EventixState) -> anyhow::Result<bool> {
        let child = self.cmd.spawn()?;
        let output = child.wait_with_output().await?;
        let status = output.status;
        let res = self.post_process(cal, state, output).await?;
        if status.success() {
            Ok(res)
        } else {
            Err(anyhow!("exited with {}", status))
        }
    }
}
