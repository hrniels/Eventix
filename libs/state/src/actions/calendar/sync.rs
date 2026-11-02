// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use anyhow::anyhow;
use eventix_ical::col::DirectoryWriteGuard;
use xdg::BaseDirectories;

use crate::{
    CollectionSettings, EventixState, State, SyncCancellation, SyncProgress, SyncResult,
    SyncerType, crypto, encrypt_password, retrieve_portal_secret, sync,
};

use super::super::Action;

struct CollectionSyncPlan {
    col_id: String,
    index: usize,
    snapshot: CollectionSettings,
    token: Option<String>,
    protection: Option<DirectoryWriteGuard>,
}

async fn run_collection_sync_op<F, Fut>(
    state: &EventixState,
    col_id: &String,
    auth_url: Option<&String>,
    cancellation: &SyncCancellation,
    run: F,
) -> anyhow::Result<sync::SyncResult>
where
    F: FnOnce(
        Arc<BaseDirectories>,
        usize,
        String,
        CollectionSettings,
        Option<String>,
        Option<String>,
        SyncCancellation,
    ) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<sync::SyncExecution>>,
{
    let preparation = async {
        let mut state = state.lock().await;
        // in case the o365 is empty (e.g., due to an earlier migration where it didn't matter
        // that it was empty), repair it here by generating a new one and storing it.
        repair_o365_password(&mut state, col_id).await?;
        let plan = prepare_collection_sync(&mut state, col_id)?;
        Ok::<_, anyhow::Error>((plan, state.xdg.clone()))
    };
    let (mut plan, xdg) = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Ok(SyncResult {
            cancelled: true,
            ..Default::default()
        }),
        result = preparation => result?,
    };

    // Backends observe cancellation themselves so subprocess cleanup completes before
    // write protection is released and partially updated files are read back into state.
    let execution = run(
        xdg,
        plan.index,
        plan.col_id.clone(),
        plan.snapshot.clone(),
        plan.token.clone(),
        auth_url.cloned(),
        cancellation.clone(),
    )
    .await;
    let interrupted = match &execution {
        Err(error) => error.is::<sync::SyncCancelled>(),
        Ok((_, _, Err(error))) => error.is::<sync::SyncCancelled>(),
        _ => false,
    };

    let mut state = state.lock().await;
    drop(plan.protection.take());
    if interrupted {
        sync::reload_collection_from_disk(&mut state, col_id, Some(&plan.snapshot))?;
        return Ok(SyncResult {
            cancelled: true,
            // A stopped subprocess may already have changed files, even without a result.
            changed: true,
            ..Default::default()
        });
    }
    let (snapshot, mut syncer, res) = execution?;
    let mut sync_res = sync::SyncResult::default();
    sync::handle_sync_result(
        &mut state,
        &plan.col_id,
        &snapshot,
        &mut syncer,
        res,
        &mut sync_res,
    )
    .await;
    Ok(sync_res)
}

fn prepare_collection_sync(
    state: &mut State,
    col_id: &String,
) -> anyhow::Result<CollectionSyncPlan> {
    let snapshot = state
        .settings()
        .collections()
        .get(col_id)
        .cloned()
        .ok_or_else(|| anyhow!("No collection '{}'", col_id))?;
    let index = state
        .settings()
        .collections()
        .keys()
        .enumerate()
        .find_map(|(idx, id)| (id == col_id).then_some(idx))
        .ok_or_else(|| anyhow!("No collection '{}'", col_id))?;
    let token = state.misc().collection_token(col_id).cloned();
    let protected_dirs = snapshot
        .all_calendars()
        .keys()
        .cloned()
        .map(Arc::new)
        .collect::<Vec<_>>();
    let protection = state
        .store_mut()
        .protect_directories(protected_dirs.clone())
        .map_err(anyhow::Error::from)?;
    Ok(CollectionSyncPlan {
        col_id: col_id.clone(),
        index,
        snapshot,
        token,
        protection: Some(protection),
    })
}

async fn repair_o365_password(state: &mut State, col_id: &String) -> anyhow::Result<()> {
    let replacement =
        state
            .settings()
            .collections()
            .get(col_id)
            .and_then(|col| match col.syncer() {
                SyncerType::O365 {
                    email,
                    read_only,
                    password,
                    time_span,
                } if password.nonce.is_empty() && password.ciphertext.is_empty() => {
                    Some((email.clone(), *read_only, time_span.clone()))
                }
                _ => None,
            });

    if let Some((email, read_only, time_span)) = replacement {
        let secret = retrieve_portal_secret().await?;
        let generated = crypto::generate_local_password();
        let password = encrypt_password(&secret, &generated)?;
        let col = state
            .settings_mut()
            .collections_mut()
            .get_mut(col_id)
            .ok_or_else(|| anyhow!("No collection '{}'", col_id))?;
        col.set_syncer(SyncerType::O365 {
            email,
            read_only,
            password,
            time_span,
        });
        state.settings().write_to_file()?;
    }

    Ok(())
}

/// Synchronizes one collection, retaining local changes if cancellation interrupts it.
///
/// Returns only after backend processes have stopped and the local store has been reconciled.
/// Cancellation preserves calendar error flags and is reported in the returned result.
#[derive(Debug)]
pub struct SyncCollection {
    pub col_id: String,
    pub auth_url: Option<String>,
    pub cancellation: SyncCancellation,
}

#[async_trait::async_trait]
impl Action<SyncResult> for SyncCollection {
    fn description(&self) -> &'static str {
        "Collection synchronized"
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<SyncResult> {
        let col_id = &self.col_id;
        let auth_url = self.auth_url.as_ref();
        let cancellation = &self.cancellation;

        run_collection_sync_op(
            state,
            col_id,
            auth_url,
            cancellation,
            sync::run_sync_from_snapshot,
        )
        .await
    }
}

/// Discovers a remote collection and returns information about it.
///
/// Cancellation stops backend work and reconciles local files before returning a cancelled
/// result, preserving existing calendar error flags.
#[derive(Debug)]
pub struct DiscoverCollection {
    pub col_id: String,
    pub auth_url: Option<String>,
    pub cancellation: SyncCancellation,
}

#[async_trait::async_trait]
impl Action<SyncResult> for DiscoverCollection {
    fn description(&self) -> &'static str {
        "Collection discovered"
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<SyncResult> {
        let col_id = &self.col_id;
        let auth_url = self.auth_url.as_ref();
        let cancellation = &self.cancellation;

        run_collection_sync_op(
            state,
            col_id,
            auth_url,
            cancellation,
            sync::run_discover_from_snapshot,
        )
        .await
    }
}

/// Reloads a collection from its remote source and refreshes local files.
///
/// Cancellation stops remaining reload stages and reconciles local files before returning a
/// cancelled result. Files already deleted or downloaded are not restored to their old state.
#[derive(Debug)]
pub struct ReloadCollection {
    pub col_id: String,
    pub auth_url: Option<String>,
    pub cancellation: SyncCancellation,
}

#[async_trait::async_trait]
impl Action<SyncResult> for ReloadCollection {
    fn description(&self) -> &'static str {
        "Collection reloaded"
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<SyncResult> {
        let col_id = &self.col_id;
        let auth_url = self.auth_url.as_ref();
        let cancellation = &self.cancellation;

        run_collection_sync_op(
            state,
            col_id,
            auth_url,
            cancellation,
            sync::run_reload_collection_from_snapshot,
        )
        .await
    }
}

/// Reloads a single calendar from its remote source and refreshes the local file.
///
/// Cancellation stops remaining reload stages and reconciles local files before returning a
/// cancelled result. Files already deleted or downloaded are not restored to their old state.
#[derive(Debug)]
pub struct ReloadCalendar {
    pub col_id: String,
    pub cal_id: String,
    pub auth_url: Option<String>,
    pub cancellation: SyncCancellation,
}

#[async_trait::async_trait]
impl Action<SyncResult> for ReloadCalendar {
    fn description(&self) -> &'static str {
        "Calendar reloaded"
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<SyncResult> {
        let col_id = &self.col_id;
        let auth_url = self.auth_url.as_ref();
        let cancellation = &self.cancellation;
        let cal_id = &self.cal_id;

        run_collection_sync_op(
            state,
            col_id,
            auth_url,
            cancellation,
            |xdg, idx, id, col, token, auth, cancellation| {
                sync::run_reload_calendar_from_snapshot(
                    xdg,
                    idx,
                    id,
                    col,
                    token,
                    auth,
                    cancellation,
                    cal_id,
                )
            },
        )
        .await
    }
}

/// Synchronizes all collections until completion or cancellation.
///
/// Keeps completed results and skips remaining collections after cancellation. Interrupted
/// collections are reconciled before returning, without counting them as completed progress.
/// Only a fully completed operation advances the last-reload timestamp.
pub struct SyncAll<'a> {
    pub auth_url: Option<String>,
    pub progress: Option<&'a dyn SyncProgress>,
    pub cancellation: SyncCancellation,
}

impl std::fmt::Debug for SyncAll<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncAll")
            .field("auth_url", &self.auth_url)
            .field("cancellation", &self.cancellation)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl Action<SyncResult> for SyncAll<'_> {
    fn description(&self) -> &'static str {
        "Synchronization completed"
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<SyncResult> {
        let auth_url = self.auth_url.as_ref();
        let progress = self.progress;
        let cancellation = &self.cancellation;

        let snapshot = async {
            let state = state.lock().await;
            state
                .settings()
                .collections()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };
        let col_ids = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Ok(SyncResult {
                cancelled: true,
                ..Default::default()
            }),
            ids = snapshot => ids,
        };

        let total = col_ids.len();
        if let Some(progress) = progress {
            progress.started(total);
        }
        let mut sync_res = sync::SyncResult::default();
        for (index, col_id) in col_ids.into_iter().enumerate() {
            let mut col_res = Box::new(SyncCollection {
                col_id: col_id.clone(),
                auth_url: auth_url.cloned(),
                cancellation: cancellation.clone(),
            })
            .execute(state)
            .await?;
            sync_res.changed |= col_res.changed;
            sync_res.cancelled |= col_res.cancelled;
            sync_res.collections.extend(col_res.collections.drain());
            sync_res.calendars.extend(col_res.calendars.drain());
            if sync_res.cancelled {
                return Ok(sync_res);
            }
            if let Some(progress) = progress {
                progress.collection_ready(&col_id, index + 1, total);
            }
        }

        let mut state = state.lock().await;
        if cancellation.is_cancelled() {
            sync_res.cancelled = true;
            return Ok(sync_res);
        }
        state.last_reload = chrono::Utc::now().naive_utc();
        Ok(sync_res)
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use eventix_ical::col::{CalDir, CalStore};

    use crate::{CalendarSettings, CollectionSettings, Misc, State, SyncerType};

    use super::prepare_collection_sync;

    #[test]
    fn prepare_collection_sync_releases_protection_on_drop() {
        let cal_id = Arc::new("cal1".to_string());
        let mut store = CalStore::default();
        store.add(CalDir::new_empty(
            cal_id.clone(),
            PathBuf::from("/tmp/cal1"),
            "Cal".into(),
            false,
        ));
        let mut state = State::new_for_test(store, Misc::new(PathBuf::default()));
        let mut col = CollectionSettings::new(SyncerType::FileSystem {
            path: "/tmp".into(),
        });
        col.all_calendars_mut()
            .insert("cal1".into(), CalendarSettings::default());
        state
            .settings_mut()
            .collections_mut()
            .insert("col1".into(), col);

        let plan = prepare_collection_sync(&mut state, &"col1".into()).unwrap();
        assert!(state.store().directory_write_protected(&cal_id));
        drop(plan);
        assert!(!state.store().directory_write_protected(&cal_id));
    }
}
