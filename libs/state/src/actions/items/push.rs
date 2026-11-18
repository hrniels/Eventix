// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Context, bail};

use super::ItemId;
use crate::actions::Action;
use crate::sync::{get_sync_cancellable, sync_snapshot};
use crate::{EventixState, State, SyncCancellation, SyncColResult, SyncerType};

/// Resolves a resource through its calendar, not the store's globally indexed UID lookup.
fn resolve_resource(state: &State, item: &ItemId) -> anyhow::Result<(String, String)> {
    let locale = state.locale();

    // determine collection from calendar name
    let (col_id, col) = state
        .settings()
        .collections()
        .iter()
        .find(|(_, col)| col.all_calendars().contains_key(&item.calendar))
        .with_context(|| {
            locale
                .translate("Calendar is no longer available")
                .to_owned()
        })?;
    if col.is_syncer_read_only() || matches!(col.syncer(), SyncerType::FileSystem { .. }) {
        bail!(
            locale
                .translate("Calendar does not support remote writes")
                .to_owned()
        );
    }

    // determine file from UID
    let file = state
        .store()
        .directory(&Arc::new(item.calendar.clone()))
        .and_then(|dir| dir.file_by_id(&item.uid))
        .with_context(|| {
            locale
                .translate("Occurrence is no longer available")
                .to_owned()
        })?;

    let filename = file
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| {
            locale
                .translate("Invalid calendar resource filename")
                .to_owned()
        })?
        .to_owned();

    Ok((col_id.clone(), filename))
}

/// Receives resource-level progress outside the application state lock.
pub trait PushProgress: Send + Sync {
    /// Reports completed resources and the total selected resource count.
    fn progress(&self, completed: usize, total: usize);

    /// Reports an individual resource outcome before advancing progress.
    fn item_ready(&self, _item: &ItemId, _outcome: &SyncColResult) {}
}

/// Individual push outcomes; missing items were not attempted.
#[derive(Debug, Default)]
pub struct PushResult {
    /// Whether cancellation stopped the operation.
    pub cancelled: bool,
    /// Outcomes identified by calendar and UID, without recurrence identities.
    pub items: Vec<(ItemId, SyncColResult)>,
}

/// Pushes selected local resources without downloading unrelated remote changes.
///
/// Resources are deduplicated by calendar and UID. Network work releases the state lock, and
/// cancellation retains completed outcomes without rolling back remote writes.
pub struct PushItems<'a> {
    pub items: Vec<ItemId>,
    pub progress: Option<&'a dyn PushProgress>,
    pub cancellation: SyncCancellation,
}

impl std::fmt::Debug for PushItems<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushItems")
            .field("items", &self.items)
            .field("cancellation", &self.cancellation)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl Action<PushResult> for PushItems<'_> {
    fn description(&self) -> &'static str {
        "Items uploaded"
    }

    async fn execute(self: Box<Self>, state: &EventixState) -> anyhow::Result<PushResult> {
        let items = &self.items;
        let progress = self.progress;
        let cancellation = &self.cancellation;
        let cancelled = || PushResult {
            cancelled: true,
            ..PushResult::default()
        };
        if cancellation.is_cancelled() {
            return Ok(cancelled());
        }

        // collect and deduplicate items to push
        let mut groups = BTreeMap::new();
        let mut seen = std::collections::HashSet::new();
        {
            let state = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Ok(cancelled()),
                state = state.lock() => state,
            };
            for item in items {
                // skip already seen items
                if !seen.insert((item.calendar.clone(), item.uid.clone())) {
                    continue;
                }

                let (col_id, filename) = resolve_resource(&state, item)?;
                let item = ItemId {
                    recurrence: None,
                    ..item.clone()
                };

                // collect a snapshot of the collection to unlock the state afterwards
                let entry = groups
                    .entry(col_id.clone())
                    .or_insert_with(|| (sync_snapshot(&state, &col_id), Vec::new()));
                entry.1.push((item, filename));
            }
        }

        let total = seen.len();
        let mut result = PushResult::default();
        if let Some(progress) = progress {
            progress.progress(0, total);
        }

        for (id, (snapshot, items)) in groups {
            if cancellation.is_cancelled() {
                break;
            }

            let (xdg, idx, mut col, token) = snapshot?;

            // Explicitly selected resources remain pushable when their calendar is hidden.
            // note that we are working on a copy and therefore do not change the actual state
            for (item, _) in &items {
                if let Some(calendar) = col.all_calendars_mut().get_mut(&item.calendar) {
                    calendar.set_enabled(true);
                }
            }

            // determine backend for this collection
            let backend =
                get_sync_cancellable(xdg, idx, id, col, token, None, cancellation.clone()).await;
            let mut backend = match backend {
                Ok(backend) => backend,
                Err(_) if cancellation.is_cancelled() => break,
                Err(error) => {
                    for (item, _) in items {
                        let outcome = SyncColResult::Error(format!("{error:#}"));
                        if let Some(progress) = progress {
                            progress.item_ready(&item, &outcome);
                        }
                        result.items.push((item, outcome));
                        if let Some(progress) = progress {
                            progress.progress(result.items.len(), total);
                        }
                    }
                    continue;
                }
            };

            for (item, filename) in items {
                // push item to remote side
                if cancellation.is_cancelled() {
                    break;
                }
                let outcome = backend.syncer.push_item(&item.calendar, &filename).await;
                if cancellation.is_cancelled() {
                    break;
                }

                let outcome =
                    outcome.unwrap_or_else(|error| SyncColResult::Error(format!("{error:#}")));
                // O365 also uses finish to retain refreshed authentication tokens.
                let mut outcome = outcome;
                if let Err(error) = backend
                    .syncer
                    .finish(&mut *state.lock().await, &mut outcome)
                {
                    outcome = SyncColResult::Error(format!("{error:#}"));
                }

                // add to results
                if let Some(progress) = progress {
                    progress.item_ready(&item, &outcome);
                }
                result.items.push((item, outcome));
                if let Some(progress) = progress {
                    progress.progress(result.items.len(), total);
                }
            }
        }

        result.cancelled = cancellation.is_cancelled();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Progress(Mutex<Vec<(usize, usize)>>);

    impl PushProgress for Progress {
        fn progress(&self, completed: usize, total: usize) {
            self.0.lock().unwrap().push((completed, total));
        }
    }

    #[test]
    fn resolution_uses_the_actual_filename_and_rejects_non_remote_calendars() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("different-name.ics"),
            concat!(
                "BEGIN:VCALENDAR\r\n",
                "VERSION:2.0\r\n",
                "BEGIN:VEVENT\r\n",
                "UID:uid\r\n",
                "DTSTART:20261103T120000Z\r\n",
                "END:VEVENT\r\n",
                "END:VCALENDAR\r\n",
            ),
        )
        .unwrap();

        let mut state = State::new(Arc::new(crate::with_test_xdg(tmp.path(), tmp.path()))).unwrap();
        state.store_mut().add(
            eventix_ical::col::CalDir::new_from_dir(
                Arc::new("calendar".into()),
                tmp.path().into(),
                "Calendar".into(),
                false,
            )
            .unwrap(),
        );

        // create collection and insert calendar
        let mut col = crate::CollectionSettings::new(SyncerType::VDirSyncer {
            email: crate::EmailAccount::new("Test".into(), "test@example.com".into()),
            url: "http://localhost/".into(),
            username: None,
            password: None,
            read_only: false,
            time_span: Default::default(),
        });
        col.all_calendars_mut()
            .insert("calendar".into(), crate::CalendarSettings::default());
        state
            .settings_mut()
            .collections_mut()
            .insert("collection".into(), col);

        // test resolve success
        let item = ItemId {
            calendar: "calendar".into(),
            uid: "uid".into(),
            recurrence: None,
        };
        assert_eq!(
            resolve_resource(&state, &item).unwrap(),
            ("collection".into(), "different-name.ics".into())
        );

        // test resolve failure
        let mut missing = item.clone();
        missing.uid = "missing".into();
        assert!(resolve_resource(&state, &missing).is_err());

        // FileSystem is not supported here
        state
            .settings_mut()
            .collections_mut()
            .get_mut("collection")
            .unwrap()
            .set_syncer(SyncerType::FileSystem {
                path: tmp.path().display().to_string(),
            });
        assert!(resolve_resource(&state, &item).is_err());
    }

    #[tokio::test]
    async fn empty_push_reports_progress_without_advancing_sync_time() {
        let tmp = tempfile::tempdir().unwrap();
        let state =
            crate::State::new(Arc::new(crate::with_test_xdg(tmp.path(), tmp.path()))).unwrap();
        let before = state.last_reload();
        let state = Arc::new(tokio::sync::Mutex::new(state));
        let progress = Progress::default();
        let result =
            crate::State::push_items(&state, &[], Some(&progress), &SyncCancellation::default())
                .await
                .unwrap();
        assert!(!result.cancelled);
        assert!(result.items.is_empty());
        assert_eq!(*progress.0.lock().unwrap(), [(0, 0)]);
        assert_eq!(state.lock().await.last_reload(), before);
    }
}
