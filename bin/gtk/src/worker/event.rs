// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Worker events, response data, and activity-aware result publication.

use eventix_state::actions::items::{ItemId, UpdateItem};
use eventix_state::auth::{AuthOutcome, AuthPrompt};
use eventix_state::{SyncProgress, SyncResult};

use super::{
    management::Request as ManagementRequest, message::OccurrencesRequest,
    subscriptions::SubscriptionId,
};
use crate::model::{
    SavedItem, Snapshot,
    activity::{Activity, Phase},
    calendars::{Backend, CalendarVisibility},
    editor::EditorOptions,
    import::PreparedImport,
    list, list_occurrences as occurrences,
    notifications::Notification,
    sidebar::SidebarSnapshot,
};

pub(crate) struct ContentResponse {
    /// Identifies the content selection this result belongs to, including on failure.
    ///
    /// The page accepts the result only if this matches its current generation.
    pub generation: u64,
    pub result: Result<Snapshot, String>,
}

pub(crate) struct ListRowsResponse {
    pub revision: u64,
    pub result: Result<Vec<(ItemId, Option<list::Row>)>, String>,
}

pub(crate) struct OccurrencesResponse {
    pub request: OccurrencesRequest,
    pub result: Result<occurrences::Page, String>,
}

/// Notifications carry identities and invalidations; snapshots are refetched separately.
pub(crate) struct ChangeSet {
    pub item: ItemId,
    pub affected: Vec<SubscriptionId>,
}

pub(crate) enum Event {
    ImportPrepared(Result<PreparedImport, String>),
    ImportFinished(Result<(), String>),
    Alarm(Notification),
    PreferencesLoaded(eventix_state::Preferences),
    PreferencesSaved(Result<eventix_state::Preferences, String>),
    PushAuthenticationRequired(String),
    PendingPushes(usize),
    Activity(Activity),
    CalendarsLoaded(Vec<CalendarVisibility>),
    SyncCollectionsLoaded(Vec<(String, Backend)>),
    ManagementLoaded(u64, Result<crate::model::calendars::Snapshot, String>),
    ManagementFinished(ManagementRequest, Result<(), String>),
    CalendarToggleFailed(String),
    EditorLoaded(u64, Result<EditorOptions, String>),
    EditorSaved(u64, Result<ChangeSet, String>),
    EditLoaded(u64, Result<(EditorOptions, Box<UpdateItem>), String>),
    EditSaved(u64, Result<(ChangeSet, SavedItem), String>),
    RemindersSaved(Result<(ChangeSet, SavedItem), String>),
    DetailsLoaded(u64, Result<SavedItem, String>),
    FilepathLoaded(Result<String, String>),
    ContentLoaded(ContentResponse),
    ListRowsLoaded(ListRowsResponse),
    OccurrencesLoaded(OccurrencesResponse),
    SidebarLoaded(Result<SidebarSnapshot, String>),
    StateLoaded,
    ReloadFailed(String),
    SyncProgress {
        completed: usize,
        total: usize,
    },
    OperationStarted {
        description: &'static str,
        determinate: bool,
    },
    OperationFinished {
        completed: bool,
        cancelled: bool,
    },
    SyncFinished(Result<SyncResult, String>),
    AuthPrompt(u64, AuthPrompt),
    AuthFinished(u64, Result<AuthOutcome, String>),
    Changed(ChangeSet),
    CommandFailed(String),
}

impl Event {
    fn failure(&self) -> Option<(&'static str, &str)> {
        let (operation, error) = match self {
            Self::ImportPrepared(result) => ("Import", result.as_ref().err()),
            Self::ImportFinished(result) => ("Import", result.as_ref().err()),
            Self::ManagementFinished(request, result) => {
                (request.description(), result.as_ref().err())
            }
            Self::CalendarToggleFailed(error) => ("Calendar visibility changed", Some(error)),
            Self::CommandFailed(error) => ("Applying changes", Some(error)),
            Self::EditorLoaded(_, result) => ("Preparing editor", result.as_ref().err()),
            Self::EditorSaved(_, result) => ("New item created", result.as_ref().err()),
            Self::EditLoaded(_, result) => ("Loading item", result.as_ref().err()),
            Self::EditSaved(_, result) => ("Item updated", result.as_ref().err()),
            Self::RemindersSaved(result) => ("Reminders updated", result.as_ref().err()),
            Self::DetailsLoaded(_, result) => ("Loading item", result.as_ref().err()),
            Self::FilepathLoaded(result) => ("Loading filepath", result.as_ref().err()),
            Self::ContentLoaded(response) => ("Loading calendar", response.result.as_ref().err()),
            Self::ListRowsLoaded(response) => ("Loading calendar", response.result.as_ref().err()),
            Self::OccurrencesLoaded(response) => {
                ("Loading calendar", response.result.as_ref().err())
            }
            Self::SidebarLoaded(result) => ("Loading sidebar", result.as_ref().err()),
            Self::ManagementLoaded(_, result) => ("Loading calendar", result.as_ref().err()),
            Self::ReloadFailed(error) => ("Loading calendar", Some(error)),
            Self::SyncFinished(result) => ("Synchronizing calendars", result.as_ref().err()),
            Self::AuthFinished(_, result) => ("Authenticating", result.as_ref().err()),
            _ => return None,
        };
        error.map(|error| (operation, error.as_str()))
    }
}

/// Publishes activity before results, so ignored stale results still leave an audit trail.
pub(super) fn publish_events(
    results: &async_channel::Sender<Event>,
    events: Vec<Event>,
    activity: Option<Activity>,
    request: u64,
) {
    // Calendar names from this response must be available when its activity is displayed.
    let events: Vec<_> = events
        .into_iter()
        .filter_map(|event| {
            if matches!(event, Event::CalendarsLoaded(_)) {
                let _ = results.send_blocking(event);
                None
            } else {
                Some(event)
            }
        })
        .collect();

    let mut failed = false;
    let mut cancelled = false;
    let mut activity = activity;
    for event in &events {
        if let Some((operation, error)) = event.failure() {
            let mutation_failed = matches!(
                event,
                Event::CalendarToggleFailed(_)
                    | Event::ManagementFinished(_, Err(_))
                    | Event::CommandFailed(_)
                    | Event::EditorSaved(..)
                    | Event::EditSaved(..)
                    | Event::RemindersSaved(_)
                    | Event::SyncFinished(_)
                    | Event::AuthFinished(..)
            );
            failed |= mutation_failed;
            let entry = if mutation_failed {
                activity.clone()
            } else {
                None
            }
            .unwrap_or_else(|| Activity::new(request, operation));
            let _ = results.send_blocking(Event::Activity(
                entry.finish(Phase::Failed, Some(error.into())),
            ));
        }

        let item = match event {
            Event::EditorSaved(_, Ok(change)) | Event::Changed(change) => Some(&change.item),
            Event::EditSaved(_, Ok((change, _))) | Event::RemindersSaved(Ok((change, _))) => {
                Some(&change.item)
            }
            _ => None,
        };

        if let (Some(activity), Some(item)) = (&mut activity, item) {
            activity.add_item(item.clone());
        }

        if let Event::SyncFinished(Ok(result)) = event {
            cancelled = result.cancelled;
            let mut errors = Vec::new();
            for (collection, outcome) in &result.collections {
                let error = match outcome {
                    eventix_state::SyncColResult::Error(error) => Some(error.clone()),
                    eventix_state::SyncColResult::AuthFailed(_) => {
                        Some("Authentication required".to_string())
                    }
                    _ => None,
                };
                if let Some(error) = error {
                    errors.push((collection.clone(), error, true));
                }
            }

            for (calendar, failed) in &result.calendars {
                if *failed {
                    errors.push((
                        calendar.clone(),
                        "Calendar synchronization failed".into(),
                        false,
                    ));
                }
            }
            errors.sort();

            for (scope, error, collection) in errors {
                failed = true;
                let mut entry = activity
                    .clone()
                    .unwrap_or_else(|| Activity::new(request, "Synchronizing calendars"));
                if collection {
                    entry.set_collection(Some(scope));
                } else {
                    entry.set_calendar(Some(scope));
                }
                let _ = results
                    .send_blocking(Event::Activity(entry.finish(Phase::Failed, Some(error))));
            }
        }
    }

    if let Some(activity) = activity {
        // Cancellation is terminal even if some collections also reported errors.
        if cancelled || !failed {
            let phase = if cancelled {
                Phase::Cancelled
            } else {
                Phase::Completed
            };
            let _ = results.send_blocking(Event::Activity(activity.finish(phase, None)));
        }
    }

    for event in events {
        let _ = results.send_blocking(event);
    }
}

pub(super) struct SessionProgress(pub(super) async_channel::Sender<Event>);

impl SyncProgress for SessionProgress {
    fn started(&self, total: usize) {
        let _ = self.0.try_send(Event::SyncProgress {
            completed: 0,
            total,
        });
    }

    fn collection_ready(&self, _collection: &str, completed: usize, total: usize) {
        let _ = self.0.try_send(Event::SyncProgress { completed, total });
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, publish_events};
    use crate::model::activity::{Activity, Phase};
    use crate::model::calendars::CalendarVisibility;

    #[test]
    fn sync_activity_keeps_partial_failures_and_cancellation_without_claiming_success() {
        let (sender, receiver) = async_channel::unbounded();
        let activity = Activity::new(7, "Synchronizing calendars");
        let result = || eventix_state::SyncResult {
            collections: [
                (
                    "Work".into(),
                    eventix_state::SyncColResult::Error("Connection\nrefused".into()),
                ),
                (
                    "Office".into(),
                    eventix_state::SyncColResult::AuthFailed("https://secret.invalid".into()),
                ),
            ]
            .into(),
            calendars: [("Personal".into(), true)].into(),
            ..Default::default()
        };
        publish_events(
            &sender,
            vec![
                Event::SyncFinished(Ok(result())),
                Event::CalendarsLoaded(vec![CalendarVisibility {
                    id: "Personal".into(),
                    name: "My calendar".into(),
                    background: "#ffffff".into(),
                    enabled: true,
                    sync_error: None,
                }]),
            ],
            Some(activity.clone()),
            8,
        );
        let Event::CalendarsLoaded(calendars) = receiver.try_recv().unwrap() else {
            panic!("calendar names must be published before the sync activity");
        };
        assert_eq!(calendars[0].name, "My calendar");
        let mut entries = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            if let Event::Activity(entry) = event {
                entries.push(entry);
            }
        }
        assert_eq!(entries.len(), 3);
        assert!(
            entries
                .iter()
                .all(|entry| entry.phase() == Phase::Failed && entry.request() == 7)
        );
        assert!(entries.iter().all(|entry| {
            !entry
                .text(eventix_locale::default().as_ref(), str::to_owned)
                .contains("secret.invalid")
        }));
        assert!(entries.iter().any(|entry| {
            entry
                .text(eventix_locale::default().as_ref(), str::to_owned)
                .contains("Collection: Work")
        }));

        publish_events(
            &sender,
            vec![Event::SyncFinished(Ok(eventix_state::SyncResult {
                cancelled: true,
                ..result()
            }))],
            Some(activity),
            9,
        );
        let mut entries = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            if let Event::Activity(entry) = event {
                entries.push(entry);
            }
        }
        assert_eq!(entries.len(), 4);
        assert_eq!(entries.last().unwrap().phase(), Phase::Cancelled);
        assert!(
            entries
                .iter()
                .all(|entry| entry.phase() != Phase::Completed)
        );
    }
}
