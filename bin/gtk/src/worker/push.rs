// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Session upload execution, resource tracking, and trailing-edge debounce.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use eventix_state::{EventixState, State, SyncCancellation, SyncerType, actions::items::ItemId};

use super::{
    event::Event,
    management::{self, Request as ManagementRequest},
    message::Message,
};
use crate::model::activity::{Activity, Phase};

type Key = (String, String);

/// Captures resources before an action and observes its persisted changes afterwards.
pub(super) struct ResourceChanges(Vec<(ItemId, Option<Vec<u8>>)>);

impl ResourceChanges {
    pub fn capture(state: &State, items: &[ItemId]) -> Self {
        Self(
            items
                .iter()
                .map(|item| (item.clone(), resource_contents(state, item)))
                .collect(),
        )
    }

    pub fn observe(mut self, queue: &mut PushQueue, state: &State, item: &ItemId) {
        if !self
            .0
            .iter()
            .any(|(candidate, _)| candidate.calendar == item.calendar && candidate.uid == item.uid)
        {
            self.0.push((item.clone(), None));
        }
        let now = Instant::now();
        for (candidate, contents) in self.0 {
            queue.observe(state, candidate, contents.as_deref(), now);
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct PendingItem {
    pub item: ItemId,
    // Queue revision at the last edit; upload batches retain this value for acknowledgement.
    pub generation: u64,
}

#[derive(Default)]
pub(super) struct PushQueue {
    entries: BTreeMap<Key, PendingItem>,
    deadline: Option<Instant>,
    last_edit: Option<Instant>,
    // Tracks additions, updates, and removals so the session can detect queue changes and
    // publish the pending count. Also supplies fresh resource generations: editing an
    // already queued resource must advance this counter even though the count stays the same.
    revision: u64,
}

/// Owns a started upload's task, cancellation, and resource generations.
pub(super) struct Upload {
    pub task: tokio::task::JoinHandle<()>,
    pub cancellation: SyncCancellation,
    pub activity: Activity,
    pub batch: Vec<PendingItem>,
}

/// Captures calendar contents rather than timestamps, excluding local-only and no-op actions.
fn resource_contents(state: &State, item: &ItemId) -> Option<Vec<u8>> {
    let file = state
        .store()
        .directory(&Arc::new(item.calendar.clone()))?
        .file_by_id(&item.uid)?;
    let mut bytes = Vec::new();
    file.calendar().write(&mut bytes).ok()?;
    Some(bytes)
}

impl PushQueue {
    pub fn set_delay(&mut self, delay: Duration) {
        if self.deadline.is_some() {
            self.deadline = self.last_edit.map(|edit| edit + delay);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn items(&self) -> Vec<PendingItem> {
        self.entries.values().cloned().collect()
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub fn due(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| deadline <= now)
    }

    pub fn start(&mut self) -> Vec<PendingItem> {
        self.deadline = None;
        self.items()
    }

    pub fn acknowledge_outcome(
        &mut self,
        batch: &[PendingItem],
        item: &ItemId,
        outcome: &eventix_state::SyncColResult,
    ) {
        if matches!(outcome, eventix_state::SyncColResult::Success(_))
            && let Some(entry) = batch.iter().find(|entry| entry.item == *item)
        {
            self.acknowledge(entry);
        }
    }

    pub fn acknowledge_sync(
        &mut self,
        state: &State,
        batch: &[PendingItem],
        result: &eventix_state::SyncResult,
    ) {
        for entry in batch {
            let covered = state.settings().collections().iter().find_map(|(id, col)| {
                col.calendars()
                    .any(|(id, _)| id == &entry.item.calendar)
                    .then(|| result.collections.get(id))
                    .flatten()
            });
            if matches!(covered, Some(eventix_state::SyncColResult::Success(_))) {
                self.acknowledge(entry);
            }
        }
    }

    pub fn management_batch(&self, state: &State, command: &ManagementRequest) -> Vec<PendingItem> {
        self.items()
            .into_iter()
            .filter(|entry| match &command.operation {
                management::Operation::Calendar(management::CalendarOperation::Reload) => {
                    entry.item.calendar == command.calendar
                }
                management::Operation::Collection(
                    management::CollectionOperation::Sync | management::CollectionOperation::Reload,
                ) => state
                    .settings()
                    .collections()
                    .get(&command.collection)
                    .is_some_and(|col| col.calendars().any(|(id, _)| id == &entry.item.calendar)),
                _ => false,
            })
            .collect()
    }

    pub fn prune(&mut self, state: &State) {
        let before = self.entries.len();
        self.entries.retain(|_, entry| {
            state
                .settings()
                .collections()
                .values()
                .find(|col| col.all_calendars().contains_key(&entry.item.calendar))
                .is_some_and(|col| {
                    !col.is_syncer_read_only()
                        && !matches!(col.syncer(), SyncerType::FileSystem { .. })
                })
                && state
                    .store()
                    .directory(&Arc::new(entry.item.calendar.clone()))
                    .is_some_and(|dir| dir.file_by_id(&entry.item.uid).is_some())
        });
        if before != self.entries.len() {
            self.revision += 1;
        }
        if self.entries.is_empty() {
            self.deadline = None;
        }
    }

    pub fn acknowledge(&mut self, item: &PendingItem) {
        let key = (item.item.calendar.clone(), item.item.uid.clone());
        // An upload confirms only its captured generation, not edits made while it was running.
        if self
            .entries
            .get(&key)
            .is_some_and(|entry| entry.generation == item.generation)
        {
            self.entries.remove(&key);
            self.revision += 1;
        }
        if self.entries.is_empty() {
            self.deadline = None;
        }
    }

    pub fn observe(
        &mut self,
        state: &State,
        mut item: ItemId,
        before: Option<&[u8]>,
        now: Instant,
    ) {
        let key = (item.calendar.clone(), item.uid.clone());
        let Some(after) = resource_contents(state, &item) else {
            // no such file anymore? remove it and update revision+deadline
            if self.entries.remove(&key).is_some() {
                self.revision += 1;
                self.last_edit = Some(now);
                self.deadline = (!self.entries.is_empty()).then_some(
                    now + Duration::from_secs(u64::from(
                        state.settings().preferences().post_delay_seconds,
                    )),
                );
            }
            return;
        };
        // if the ICS file has not changed, there is nothing to push
        if before == Some(after.as_slice()) {
            return;
        }

        // get collection and ensure we need to push something
        let Some((col, _)) = state.settings().calendar(&item.calendar) else {
            return;
        };
        if col.is_syncer_read_only() || matches!(col.syncer(), SyncerType::FileSystem { .. }) {
            return;
        }

        // add new entry to push
        item.recurrence = None;
        self.revision += 1;
        self.entries.insert(
            key,
            PendingItem {
                item,
                generation: self.revision,
            },
        );
        self.last_edit = Some(now);
        self.deadline = Some(
            now + Duration::from_secs(u64::from(state.settings().preferences().post_delay_seconds)),
        );
    }

    /// Reconciles upload outcomes and returns completion and authentication events.
    pub fn finish_upload(
        &mut self,
        state: Option<&State>,
        batch: &[PendingItem],
        activity: Option<Activity>,
        result: Result<eventix_state::PushResult, String>,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        let cancelled = result.as_ref().is_ok_and(|result| result.cancelled);
        let mut error = result.as_ref().err().cloned();
        let mut authentication = None;
        if let Ok(result) = &result {
            for (item, outcome) in &result.items {
                self.acknowledge_outcome(batch, item, outcome);

                match outcome {
                    eventix_state::SyncColResult::Success(_) => {}
                    eventix_state::SyncColResult::Error(message) => error = Some(message.clone()),
                    eventix_state::SyncColResult::AuthFailed(_) => {
                        error = Some("Authentication required".into());
                        if authentication.is_none() {
                            authentication = state.and_then(|state| {
                                state.settings().collections().iter().find_map(|(id, col)| {
                                    col.all_calendars()
                                        .contains_key(&item.calendar)
                                        .then(|| id.clone())
                                })
                            });
                        }
                    }
                }
            }
        }

        if let Some(activity) = activity {
            let phase = if cancelled {
                Phase::Cancelled
            } else if error.is_some() {
                Phase::Failed
            } else {
                Phase::Completed
            };
            events.push(Event::Activity(activity.finish(phase, error.clone())));
        }

        events.push(Event::OperationFinished {
            completed: error.is_none(),
            cancelled,
        });
        if !cancelled && let Some(collection) = authentication {
            events.push(Event::PushAuthenticationRequired(collection));
        }
        events
    }

    /// Starts queued uploads and returns their handle without owning session scheduling.
    pub fn begin_upload(
        &mut self,
        state: EventixState,
        runtime: &tokio::runtime::Runtime,
        requests: async_channel::Sender<Message>,
        results: async_channel::Sender<Event>,
        request: u64,
    ) -> Option<Upload> {
        let batch = self.start();
        if batch.is_empty() {
            return None;
        }

        let items = batch
            .iter()
            .map(|entry| entry.item.clone())
            .collect::<Vec<_>>();
        let cancellation = SyncCancellation::default();
        let token = cancellation.clone();
        let progress = PushSessionProgress {
            results: results.clone(),
            requests: requests.clone(),
        };

        let mut activity = Activity::new(request, "Items uploaded");
        activity.set_items(items.clone());

        let _ = results.send_blocking(Event::Activity(activity.clone()));
        let _ = results.send_blocking(Event::OperationStarted {
            description: "Pushing items…",
            determinate: true,
        });

        let task = runtime.spawn(async move {
            let result = tokio::spawn(async move {
                State::push_items(&state, &items, Some(&progress), &token).await
            })
            .await
            .map_err(anyhow::Error::from)
            .and_then(|result| result)
            .map_err(|error| format!("{error:#}"));
            let _ = requests.send(Message::PushFinished(result)).await;
        });

        Some(Upload {
            task,
            cancellation,
            activity,
            batch,
        })
    }
}

struct PushSessionProgress {
    results: async_channel::Sender<Event>,
    requests: async_channel::Sender<Message>,
}

impl eventix_state::PushProgress for PushSessionProgress {
    fn progress(&self, completed: usize, total: usize) {
        let _ = self
            .results
            .try_send(Event::SyncProgress { completed, total });
    }

    fn item_ready(&self, item: &ItemId, outcome: &eventix_state::SyncColResult) {
        let _ = self
            .requests
            .try_send(Message::PushItemReady(item.clone(), outcome.clone()));
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn fixture() -> (tempfile::TempDir, State, ItemId) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("different-filename.ics"),
            concat!(
                "BEGIN:VCALENDAR\r\n",
                "VERSION:2.0\r\n",
                "BEGIN:VEVENT\r\n",
                "UID:uid\r\n",
                "SUMMARY:Summary\r\n",
                "DTSTART:20261103T120000Z\r\n",
                "DTEND:20261103T130000Z\r\n",
                "END:VEVENT\r\n",
                "END:VCALENDAR\r\n",
            ),
        )
        .unwrap();

        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();

        state.store_mut().add(
            eventix_ical::col::CalDir::new_from_dir(
                Arc::new("calendar".into()),
                tmp.path().into(),
                "Calendar".into(),
                false,
            )
            .unwrap(),
        );

        let mut col = eventix_state::CollectionSettings::new(SyncerType::VDirSyncer {
            url: "http://localhost/".into(),
            username: None,
            read_only: false,
            email: eventix_state::EmailAccount::new("Test".into(), "test@example.com".into()),
            password: None,
            time_span: Default::default(),
        });

        let mut cal = eventix_state::CalendarSettings::default();
        cal.set_enabled(true);
        col.all_calendars_mut().insert("calendar".into(), cal);
        state
            .settings_mut()
            .collections_mut()
            .insert("remote".into(), col);

        (tmp, state, entry(0).item)
    }

    #[test]
    fn resource_changes_reset_the_shared_deadline_and_deduplicate_occurrences() {
        let (_tmp, state, mut item) = fixture();
        let now = Instant::now();

        let mut queue = PushQueue::default();
        queue.observe(&state, item.clone(), None, now);
        assert_eq!(queue.items().len(), 1);
        assert!(!queue.due(now + Duration::from_secs(4)));
        assert!(queue.due(now + Duration::from_secs(5)));

        // An occurrence edit uploads the same resource and restarts the shared debounce.
        item.recurrence = Some(eventix_ical::objects::CalDate::Date(
            NaiveDate::from_ymd_opt(2026, 11, 4).unwrap(),
            eventix_ical::objects::CalDateType::Exclusive,
        ));
        queue.observe(&state, item, None, now + Duration::from_secs(3));

        assert_eq!(queue.items().len(), 1);
        assert!(queue.items()[0].item.recurrence.is_none());
        assert!(!queue.due(now + Duration::from_secs(5)));
        assert!(queue.due(now + Duration::from_secs(8)));
    }

    #[test]
    fn configured_delay_and_changes_preserve_the_last_edit_and_active_uploads() {
        let (_tmp, mut state, item) = fixture();
        let mut preferences = state.settings().preferences();
        preferences.post_delay_seconds = 30;
        state.settings_mut().set_preferences(preferences).unwrap();
        let mut queue = PushQueue::default();
        let now = Instant::now();
        queue.observe(&state, item.clone(), None, now);
        assert_eq!(queue.deadline(), Some(now + Duration::from_secs(30)));
        queue.set_delay(Duration::from_secs(60));
        assert_eq!(queue.deadline(), Some(now + Duration::from_secs(60)));
        queue.set_delay(Duration::ZERO);
        assert!(queue.due(now));
        queue.start();
        queue.set_delay(Duration::from_secs(10));
        assert_eq!(queue.deadline(), None);
        preferences.post_delay_seconds = 0;
        state.settings_mut().set_preferences(preferences).unwrap();
        queue.observe(&state, item, None, now);
        assert!(queue.due(now));
    }

    #[test]
    fn unchanged_resources_do_not_enqueue_or_reset_the_timer() {
        let (_tmp, state, item) = fixture();
        let contents = resource_contents(&state, &item).unwrap();
        let mut queue = PushQueue::default();
        let now = Instant::now();

        queue.observe(&state, item.clone(), Some(&contents), now);
        assert!(queue.items().is_empty());

        // A no-op must not delay an upload that is already pending.
        queue.observe(&state, item.clone(), None, now);

        queue.observe(&state, item, Some(&contents), now + Duration::from_secs(3));
        assert_eq!(queue.deadline(), Some(now + Duration::from_secs(5)));
    }

    #[test]
    fn filesystem_and_read_only_collections_are_not_enqueued() {
        let (_tmp, mut state, item) = fixture();
        let mut queue = PushQueue::default();

        // Remote resources are only eligible when their collection allows writes.
        let col = state
            .settings_mut()
            .collections_mut()
            .get_mut("remote")
            .unwrap();
        let mut syncer = col.syncer().clone();
        if let SyncerType::VDirSyncer { read_only, .. } = &mut syncer {
            *read_only = true;
        }

        col.set_syncer(syncer);
        queue.observe(&state, item.clone(), None, Instant::now());
        assert!(queue.items().is_empty());

        // Filesystem collections have no remote upload target, even when writable.
        let col = state
            .settings_mut()
            .collections_mut()
            .get_mut("remote")
            .unwrap();
        col.set_syncer(SyncerType::FileSystem {
            path: "/unused".into(),
        });

        queue.observe(&state, item, None, Instant::now());
        assert!(queue.items().is_empty());
    }

    fn entry(generation: u64) -> PendingItem {
        PendingItem {
            item: ItemId {
                calendar: "calendar".into(),
                uid: "uid".into(),
                recurrence: None,
            },
            generation,
        }
    }

    #[test]
    fn acknowledgement_keeps_newer_edits_and_start_disarms_deadline() {
        let mut queue = PushQueue::default();
        let old = entry(1);
        let new = entry(2);

        queue
            .entries
            .insert(("calendar".into(), "uid".into()), new.clone());
        queue.deadline = Some(Instant::now());
        assert!(queue.due(Instant::now()));

        // Starting a batch disarms the timer but retains entries until confirmation.
        assert_eq!(queue.start().len(), 1);
        assert!(queue.deadline().is_none());

        // Confirmation of an earlier upload must not discard a later edit.
        queue.acknowledge(&old);
        assert_eq!(queue.items().len(), 1);

        queue.acknowledge(&new);
        assert!(queue.items().is_empty());
    }

    #[test]
    fn resource_changes_ignore_unchanged_sources_and_include_new_returned_items() {
        let (_tmp, state, item) = fixture();
        let mut queue = PushQueue::default();

        // An affected source is not necessarily changed, for example when copying an item.
        ResourceChanges::capture(&state, std::slice::from_ref(&item))
            .observe(&mut queue, &state, &item);
        assert_eq!(queue.len(), 0);

        // Newly created resources are returned by the action but absent from its input snapshot.
        ResourceChanges::capture(&state, &[]).observe(&mut queue, &state, &item);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.items()[0].item, item);
    }

    #[test]
    fn item_outcomes_only_acknowledge_successful_matching_generations() {
        let (_tmp, state, item) = fixture();
        let mut queue = PushQueue::default();
        queue.observe(&state, item.clone(), None, Instant::now());

        let batch = queue.start();

        // Both ordinary failures and authentication failures leave the resource pending.
        for outcome in [
            eventix_state::SyncColResult::Error("failed".into()),
            eventix_state::SyncColResult::AuthFailed("auth".into()),
        ] {
            queue.acknowledge_outcome(&batch, &item, &outcome);
            assert_eq!(queue.len(), 1);
        }

        // Even a successful upload cannot acknowledge edits made after its batch was captured.
        queue.observe(&state, item.clone(), None, Instant::now());
        let success = eventix_state::SyncColResult::Success(false);
        queue.acknowledge_outcome(&batch, &item, &success);
        assert_eq!(queue.len(), 1);

        queue.acknowledge_outcome(&queue.items(), &item, &success);
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn sync_acknowledgement_requires_success_and_an_enabled_calendar() {
        let (_tmp, mut state, item) = fixture();
        let mut queue = PushQueue::default();
        queue.observe(&state, item.clone(), None, Instant::now());
        let batch = queue.items();

        // Missing and failed collection outcomes provide no confirmation of an upload.
        let mut result = eventix_state::SyncResult::default();
        queue.acknowledge_sync(&state, &batch, &result);
        assert_eq!(queue.len(), 1);

        result.collections.insert(
            "remote".into(),
            eventix_state::SyncColResult::Error("failed".into()),
        );
        queue.acknowledge_sync(&state, &batch, &result);
        assert_eq!(queue.len(), 1);

        result.collections.insert(
            "remote".into(),
            eventix_state::SyncColResult::Success(false),
        );
        queue.acknowledge_sync(&state, &batch, &result);
        assert_eq!(queue.len(), 0);

        // Full sync uses the same generation protection as individual uploads.
        queue.observe(&state, item, None, Instant::now());
        queue.acknowledge_sync(&state, &batch, &result);
        assert_eq!(queue.len(), 1);

        // A successful collection sync does not cover calendars excluded from that sync.
        let batch = queue.items();
        state
            .settings_mut()
            .collections_mut()
            .get_mut("remote")
            .unwrap()
            .all_calendars_mut()
            .get_mut("calendar")
            .unwrap()
            .set_enabled(false);
        queue.acknowledge_sync(&state, &batch, &result);
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn management_batches_only_cover_reloaded_or_synchronized_calendars() {
        let (_tmp, state, item) = fixture();
        let mut queue = PushQueue::default();
        queue.observe(&state, item, None, Instant::now());

        let mut request = ManagementRequest {
            collection: "remote".into(),
            calendar: "calendar".into(),
            folder: String::new(),
            configured: true,
            operation: management::Operation::Calendar(management::CalendarOperation::Reload),
        };
        assert_eq!(queue.management_batch(&state, &request).len(), 1);

        // Calendar-level reloads only cover resources belonging to that calendar.
        request.calendar = "other".into();
        assert!(queue.management_batch(&state, &request).is_empty());

        // Collection operations cover their enabled calendars, not the request's calendar field.
        for operation in [
            management::CollectionOperation::Sync,
            management::CollectionOperation::Reload,
        ] {
            request.operation = management::Operation::Collection(operation);
            assert_eq!(queue.management_batch(&state, &request).len(), 1);
        }

        // Discovery alone does not upload or reload resource contents.
        request.operation =
            management::Operation::Collection(management::CollectionOperation::Discover);
        assert!(queue.management_batch(&state, &request).is_empty());
    }

    #[test]
    fn upload_completion_acknowledges_success_and_reports_activity() {
        let (_tmp, state, item) = fixture();
        let mut queue = PushQueue::default();
        queue.observe(&state, item.clone(), None, Instant::now());
        let batch = queue.start();

        let events = queue.finish_upload(
            Some(&state),
            &batch,
            Some(Activity::new(1, "Items uploaded")),
            Ok(eventix_state::PushResult {
                cancelled: false,
                items: vec![(item, eventix_state::SyncColResult::Success(true))],
            }),
        );

        assert_eq!(queue.len(), 0);
        assert!(matches!(
            events.as_slice(),
            [Event::Activity(activity), Event::OperationFinished {
                completed: true,
                cancelled: false,
            }] if activity.phase() == Phase::Completed
        ));
    }

    #[test]
    fn upload_auth_failure_retains_items_and_cancellation_suppresses_authentication() {
        let (_tmp, state, item) = fixture();
        let mut queue = PushQueue::default();
        queue.observe(&state, item.clone(), None, Instant::now());
        let batch = queue.start();

        for cancelled in [false, true] {
            let events = queue.finish_upload(
                Some(&state),
                &batch,
                Some(Activity::new(1, "Items uploaded")),
                Ok(eventix_state::PushResult {
                    cancelled,
                    items: vec![(
                        item.clone(),
                        eventix_state::SyncColResult::AuthFailed("auth".into()),
                    )],
                }),
            );

            assert_eq!(queue.len(), 1);

            let Event::Activity(activity) = &events[0] else {
                panic!("expected completion activity");
            };
            assert_eq!(
                activity.phase(),
                if cancelled {
                    Phase::Cancelled
                } else {
                    Phase::Failed
                }
            );
            assert!(matches!(
                &events[1],
                Event::OperationFinished { completed: false, cancelled: actual }
                    if *actual == cancelled
            ));

            // Cancellation must not trigger an authentication prompt for the failed upload.
            if cancelled {
                assert_eq!(events.len(), 2);
            } else {
                assert!(matches!(
                    &events[2],
                    Event::PushAuthenticationRequired(collection) if collection == "remote"
                ));
            }
        }
    }
}
