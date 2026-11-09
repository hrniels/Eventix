// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serialized queries and mutations with independent background sync; never accesses GTK objects.

use std::{collections::VecDeque, fmt, sync::Arc, thread};

use chrono::{Duration, NaiveDate, Utc};
use eventix_ical::objects::CalCompType;
use eventix_state::actions::{
    Action,
    items::{CreateItem, EditScope, ItemId, UpdateItem, UpdatePersonalAlarms},
};
use eventix_state::auth::{AuthOutcome, AuthPrompt, monitor_o365_auth};
use eventix_state::{EventixState, State, SyncCancellation, SyncProgress, SyncResult};
use eventix_view::{MonthSnapshot, WeekSnapshot};
use eventix_view::{editor::EditorOptions, sidebar::SidebarSnapshot};

use super::activity::{Activity, Phase};
use super::calendars::{self, CalendarVisibility};
use super::management::{self, Request as ManagementRequest};
use super::push::{PendingItem, PushQueue, ResourceChanges};
use super::query::{Query, SubscriptionId, Subscriptions};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum View {
    #[default]
    Week,
    Month,
    List,
}

pub(crate) enum Snapshot {
    Week(WeekSnapshot),
    Month(MonthSnapshot),
    List(eventix_view::list::Snapshot),
}

#[derive(Clone, Debug)]
pub(crate) struct ContentRequest {
    /// Identifies the page's content selection; incremented for each show-content request.
    ///
    /// Responses echo this value so the page can reject results superseded by navigation.
    /// It also identifies the installed calendar subscription, surviving refreshes and reloads.
    pub generation: u64,
    pub date: Option<NaiveDate>,
    pub view: View,
    pub filter: eventix_view::list::Filter,
}

pub(crate) struct ContentResponse {
    /// Identifies the content selection this result belongs to, including on failure.
    ///
    /// The page accepts the result only if this matches its current generation.
    pub generation: u64,
    pub result: Result<Snapshot, String>,
}

pub(crate) struct ListRowsRequest {
    /// Identifies the view's index revision so late row data cannot update a newer list.
    pub revision: u64,
    pub items: Vec<ItemId>,
}

impl fmt::Debug for ListRowsRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("ListRowsRequest");
        debug.field("revision", &self.revision);
        if self.items.len() > 2 {
            debug.field("item-count", &self.items.len());
        } else {
            debug.field("items", &self.items);
        }
        debug.finish()
    }
}

pub(crate) struct ListRowsResponse {
    pub revision: u64,
    pub result: Result<Vec<(ItemId, Option<eventix_view::list::Row>)>, String>,
}

#[derive(Clone, Debug)]
pub(crate) struct OccurrencesRequest {
    pub revision: u64,
    pub token: u64,
    pub item: ItemId,
    pub cursor: eventix_view::list::occurrences::Cursor,
    pub direction: eventix_view::list::occurrences::Direction,
    pub count: usize,
}

pub(crate) struct OccurrencesResponse {
    pub request: OccurrencesRequest,
    pub result: Result<eventix_view::list::occurrences::Page, String>,
}

/// Notifications carry identities and invalidations; snapshots are refetched separately.
pub(crate) struct ChangeSet {
    pub item: ItemId,
    pub affected: Vec<SubscriptionId>,
}

pub(crate) enum Event {
    Alarm(super::alarms::Notification),
    PreferencesLoaded(eventix_state::Preferences),
    PreferencesSaved(Result<eventix_state::Preferences, String>),
    PushAuthenticationRequired(String),
    PendingPushes(usize),
    Activity(Activity),
    CalendarsLoaded(Vec<CalendarVisibility>),
    SyncCollectionsLoaded(Vec<(String, eventix_view::calendars::Backend)>),
    ManagementLoaded(u64, Result<eventix_view::calendars::Snapshot, String>),
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

pub(crate) enum SavedItem {
    Event(Box<eventix_view::Event>),
    Task(Box<eventix_view::sidebar::SidebarRow>),
}

#[derive(Debug)]
pub(super) enum Message {
    SavePreferences(eventix_state::Preferences),
    PushItemReady(ItemId, eventix_state::SyncColResult),
    PushFinished(Result<eventix_state::PushResult, String>),
    ManageCalendar(ManagementRequest),
    ToggleCalendar(String),
    PrepareEditor(u64, CalCompType),
    CreateItem(u64, Box<CreateItem>),
    LoadEdit(u64, ItemId, EditScope),
    SaveEdit(u64, Box<UpdateItem>),
    SaveReminders(ItemId, UpdatePersonalAlarms),
    LoadDetails(u64, ItemId, CalCompType),
    LoadFilepath(ItemId),
    ShowContent(ContentRequest),
    LoadManagement(u64),
    LoadListRows(ListRowsRequest),
    LoadOccurrences(OccurrencesRequest),
    RefreshSidebar,
    RefreshSubscriptions(Vec<SubscriptionId>),
    Reload,
    SyncAll,
    SyncCollection(String),
    LoadSyncCollections,
    CancelSync,
    Authenticate(u64, String),
    CancelAuthentication(u64),
    AuthenticationFinished(u64, Result<AuthOutcome, String>),
    SyncFinished(Result<SyncResult, String>),
    Execute(Box<dyn Action>),
    ManagementCompleted(ManagementRequest, Result<bool, String>),
}

impl Message {
    fn activity(&self, request: u64) -> Option<Activity> {
        let (operation, items, calendar) = match self {
            Self::ManageCalendar(request) => (
                request.description(),
                vec![],
                Some(
                    if matches!(
                        request.operation,
                        management::Operation::Collection(_)
                            | management::Operation::Calendar(
                                management::CalendarOperation::Create(_)
                            )
                    ) {
                        request.collection.clone()
                    } else {
                        request.calendar.clone()
                    },
                ),
            ),
            Self::ToggleCalendar(calendar) => (
                "Calendar visibility changed",
                vec![],
                Some(calendar.clone()),
            ),
            Self::CreateItem(_, item) => (item.description(), vec![], Some(item.calendar.clone())),
            Self::SaveEdit(_, item) => (item.description(), vec![item.item.clone()], None),
            Self::SaveReminders(item, _) => ("Reminders updated", vec![item.clone()], None),
            Self::Execute(action) => (action.description(), action.affected_items().to_vec(), None),
            Self::SyncAll => ("Synchronizing calendars", vec![], None),
            Self::SyncCollection(collection) => {
                ("Synchronizing calendars", vec![], Some(collection.clone()))
            }
            Self::Authenticate(_, collection) => {
                ("Authenticating", vec![], Some(collection.clone()))
            }
            _ => return None,
        };
        let mut activity = Activity::new(request, operation);
        activity.set_items(items);
        if matches!(self, Self::Authenticate(..) | Self::SyncCollection(_))
            || matches!(
                self,
                Self::ManageCalendar(command)
                    if matches!(
                        command.operation,
                        management::Operation::Collection(_)
                            | management::Operation::Calendar(management::CalendarOperation::Create(_))
                    )
            )
        {
            activity.set_collection(calendar);
        } else {
            activity.set_calendar(calendar);
        }
        Some(activity)
    }
}

/// Publishes activity before results, so ignored stale results still leave an audit trail.
fn publish_events(
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

/// Owns the request channel, closing the worker when the page stops or is dropped.
pub(crate) struct CalendarSession {
    requests: async_channel::Sender<Message>,
}

impl CalendarSession {
    pub fn start(prefix: String) -> (Self, async_channel::Receiver<Event>) {
        let xdg = Arc::new(xdg::BaseDirectories::with_prefix(prefix));
        Self::start_worker(Worker::new(xdg))
    }

    #[cfg(test)]
    pub(crate) fn test_start(state: State) -> (Self, async_channel::Receiver<Event>) {
        let mut worker = Worker::new(Arc::new(state.xdg().clone()));
        worker.state = Some(Arc::new(tokio::sync::Mutex::new(state)));
        Self::start_worker(worker)
    }

    fn start_worker(worker: Worker) -> (Self, async_channel::Receiver<Event>) {
        let (requests, rx) = async_channel::unbounded::<Message>();
        let (results, responses) = async_channel::unbounded();
        let worker_requests = requests.clone();

        thread::spawn(move || worker.run(rx, worker_requests, results));

        (Self { requests }, responses)
    }

    pub fn toggle_calendar(&self, id: String) {
        let _ = self.requests.try_send(Message::ToggleCalendar(id));
    }

    pub fn save_preferences(&self, preferences: eventix_state::Preferences) -> bool {
        self.requests
            .try_send(Message::SavePreferences(preferences))
            .is_ok()
    }

    pub fn manage_calendar(&self, request: ManagementRequest) {
        let _ = self.requests.try_send(Message::ManageCalendar(request));
    }

    pub fn load_filepath(&self, item: ItemId) {
        let _ = self.requests.try_send(Message::LoadFilepath(item));
    }

    pub fn load_details(&self, id: u64, item: ItemId, kind: CalCompType) {
        let _ = self.requests.try_send(Message::LoadDetails(id, item, kind));
    }

    pub fn save_reminders(&self, selected: ItemId, command: UpdatePersonalAlarms) {
        let _ = self
            .requests
            .try_send(Message::SaveReminders(selected, command));
    }

    pub fn load_edit(&self, id: u64, item: ItemId, scope: EditScope) {
        let _ = self.requests.try_send(Message::LoadEdit(id, item, scope));
    }

    pub fn save_edit(&self, id: u64, edit: Box<UpdateItem>) {
        let _ = self.requests.try_send(Message::SaveEdit(id, edit));
    }

    pub fn prepare_editor(&self, id: u64, kind: CalCompType) {
        let _ = self.requests.try_send(Message::PrepareEditor(id, kind));
    }

    pub fn create_item(&self, id: u64, item: Box<CreateItem>) {
        let _ = self.requests.try_send(Message::CreateItem(id, item));
    }

    pub fn show_content(&self, request: ContentRequest) {
        let _ = self.requests.try_send(Message::ShowContent(request));
    }

    pub fn load_management(&self, generation: u64) {
        let _ = self.requests.try_send(Message::LoadManagement(generation));
    }

    pub fn load_list_rows(&self, request: ListRowsRequest) {
        let _ = self.requests.try_send(Message::LoadListRows(request));
    }

    pub fn load_occurrences(&self, request: OccurrencesRequest) {
        let _ = self.requests.try_send(Message::LoadOccurrences(request));
    }

    pub fn refresh_sidebar(&self) {
        let _ = self.requests.try_send(Message::RefreshSidebar);
    }

    pub fn sync_all(&self) -> bool {
        self.requests.try_send(Message::SyncAll).is_ok()
    }

    pub fn sync_collection(&self, collection: String) -> bool {
        self.requests
            .try_send(Message::SyncCollection(collection))
            .is_ok()
    }

    pub fn load_sync_collections(&self) {
        let _ = self.requests.try_send(Message::LoadSyncCollections);
    }

    pub fn cancel_sync(&self) {
        let _ = self.requests.try_send(Message::CancelSync);
    }

    pub fn authenticate(&self, id: u64, collection: String) {
        let _ = self
            .requests
            .try_send(Message::Authenticate(id, collection));
    }

    pub fn cancel_authentication(&self, id: u64) {
        let _ = self.requests.try_send(Message::CancelAuthentication(id));
    }

    pub fn execute(&self, command: Box<dyn Action>) {
        let _ = self.requests.try_send(Message::Execute(command));
    }

    pub fn refresh_subscriptions(&self, subscriptions: Vec<SubscriptionId>) {
        let _ = self
            .requests
            .try_send(Message::RefreshSubscriptions(subscriptions));
    }
}

impl Drop for CalendarSession {
    fn drop(&mut self) {
        self.requests.close();
    }
}

#[derive(Clone)]
struct ContentSubscription {
    /// Retains the selection's generation when rebuilding its snapshot or query.
    generation: u64,
    date: NaiveDate,
    view: View,
    filter: eventix_view::list::Filter,
}

impl ContentSubscription {
    fn query(&self, state: &eventix_state::State) -> anyhow::Result<Query> {
        let (start, end) = match self.view {
            View::List => return Ok(Query::List),
            View::Week => {
                let start = eventix_view::week_start(self.date)
                    .ok_or_else(|| anyhow::anyhow!("Week start out of range"))?;
                let end = start
                    .checked_add_signed(Duration::days(7))
                    .ok_or_else(|| anyhow::anyhow!("Week end out of range"))?;

                (start, end)
            }
            View::Month => eventix_view::month_range(self.date)
                .ok_or_else(|| anyhow::anyhow!("Month out of range"))?,
        };

        let at_midnight = |date: NaiveDate| {
            eventix_ical::util::resolve_local_time(
                state.timezone(),
                date.and_hms_opt(0, 0, 0).unwrap(),
            )
        };

        Ok(Query::Events {
            start: at_midnight(start),
            end: at_midnight(end),
        })
    }

    fn snapshot(&self, state: &eventix_state::State) -> anyhow::Result<Snapshot> {
        match self.view {
            View::Week => WeekSnapshot::load(state, self.date).map(Snapshot::Week),
            View::Month => MonthSnapshot::load(state, self.date).map(Snapshot::Month),
            View::List => Ok(Snapshot::List(eventix_view::list::Snapshot::load(
                state,
                &self.filter,
            ))),
        }
    }
}

struct Worker {
    xdg: Arc<xdg::BaseDirectories>,
    state: Option<EventixState>,
    content: Option<ContentSubscription>,
    subscriptions: Subscriptions,
    pushes: PushQueue,
}

struct RunningSync {
    _task: tokio::task::JoinHandle<()>,
    cancellation: SyncCancellation,
    activity: Activity,
    management: bool,
    batch: Vec<PendingItem>,
}

struct AuthenticationTask(tokio::task::JoinHandle<()>);

impl Drop for AuthenticationTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct RunningAuthentication {
    id: u64,
    task: AuthenticationTask,
    activity: Activity,
}

/// Owns the runtime, notification channels, and independently running requests.
struct WorkerRuntime {
    runtime: tokio::runtime::Runtime,
    requests: async_channel::Sender<Message>,
    results: async_channel::Sender<Event>,
    sync: Option<RunningSync>,
    authentication: Option<RunningAuthentication>,
    pending_operations: VecDeque<(Message, u64)>,
}

impl WorkerRuntime {
    fn new(
        requests: async_channel::Sender<Message>,
        results: async_channel::Sender<Event>,
    ) -> Option<Self> {
        match tokio::runtime::Runtime::new() {
            Ok(runtime) => Some(Self {
                runtime,
                requests,
                results,
                sync: None,
                authentication: None,
                pending_operations: VecDeque::new(),
            }),
            Err(error) => {
                let _ = results.send_blocking(Event::Activity(
                    Activity::new(0, "Starting session")
                        .finish(Phase::Failed, Some(error.to_string())),
                ));
                let _ = results.send_blocking(Event::SyncFinished(Err(error.to_string())));
                None
            }
        }
    }

    fn cancel_authentication(&mut self, id: u64) {
        if !self
            .authentication
            .as_ref()
            .is_some_and(|task| task.id == id)
        {
            return;
        }
        if let Some(authentication) = self.authentication.take() {
            drop(authentication.task);
            let _ = self.results.send_blocking(Event::Activity(
                authentication.activity.finish(Phase::Cancelled, None),
            ));
        }
    }

    fn finish_authentication(&mut self, id: u64, result: Result<AuthOutcome, String>) {
        if !self
            .authentication
            .as_ref()
            .is_some_and(|task| task.id == id)
        {
            return;
        }
        let Some(authentication) = self.authentication.take() else {
            return;
        };
        let phase = match &result {
            Ok(AuthOutcome::Complete) => Phase::Completed,
            _ => Phase::Failed,
        };
        let error = match &result {
            Ok(AuthOutcome::Complete) => None,
            Ok(AuthOutcome::Declined) => Some("Authentication declined".into()),
            Ok(AuthOutcome::Expired) => Some("Authentication expired".into()),
            Err(error) => Some(error.clone()),
        };
        let _ = self.results.send_blocking(Event::Activity(
            authentication.activity.finish(phase, error),
        ));
        let _ = self.results.send_blocking(Event::AuthFinished(id, result));
    }
}

impl Worker {
    fn new(xdg: Arc<xdg::BaseDirectories>) -> Self {
        Self {
            xdg,
            state: None,
            content: None,
            subscriptions: Subscriptions::default(),
            pushes: PushQueue::default(),
        }
    }

    /// Processes queued requests in order while synchronization and authentication run separately.
    fn run(
        mut self,
        receiver: async_channel::Receiver<Message>,
        requests: async_channel::Sender<Message>,
        results: async_channel::Sender<Event>,
    ) {
        let Some(mut runtime) = WorkerRuntime::new(requests, results) else {
            return;
        };
        let mut request = 0;
        let mut next_alarm_check = std::time::Instant::now();

        // Queries and commands are ordered. Never discard a queued mutation.
        loop {
            if std::time::Instant::now() >= next_alarm_check {
                if let Ok(mut state) = self.state() {
                    for notification in super::alarms::check(&mut state, Utc::now()) {
                        let _ = runtime.results.send_blocking(Event::Alarm(notification));
                    }
                }
                next_alarm_check = std::time::Instant::now() + std::time::Duration::from_secs(30);
            }
            let push_deadline = self.pushes.deadline().filter(|_| runtime.sync.is_none());
            let deadline =
                push_deadline.map_or(next_alarm_check, |deadline| deadline.min(next_alarm_check));
            let message = runtime.runtime.block_on(async {
                tokio::select! {
                    message = receiver.recv() => message.ok(),
                    _ = tokio::time::sleep_until(deadline.into()) => None,
                }
            });
            let Some(message) = message else {
                if receiver.is_closed() {
                    return;
                }
                if push_deadline.is_some_and(|deadline| deadline <= std::time::Instant::now()) {
                    request += 1;
                    self.begin_push(&mut runtime, request);
                }
                continue;
            };
            request += 1;
            self.dispatch(&mut runtime, message, request);
            if runtime.results.is_closed() {
                return;
            }
        }
    }

    fn dispatch(&mut self, runtime: &mut WorkerRuntime, message: Message, request: u64) {
        let revision = self.pushes.revision();
        let reconcile = matches!(
            &message,
            Message::ManagementCompleted(..)
                | Message::ManageCalendar(..)
                | Message::Reload
                | Message::SyncFinished(..)
        );
        match message {
            Message::PushItemReady(item, outcome) => {
                if let Some(running) = &runtime.sync {
                    self.pushes
                        .acknowledge_outcome(&running.batch, &item, &outcome);
                }
            }
            Message::PushFinished(result) => {
                self.finish_push(runtime, result);
                self.next_operation(runtime);
            }
            Message::ManageCalendar(command) if runtime.sync.is_some() => {
                runtime
                    .pending_operations
                    .push_back((Message::ManageCalendar(command), request));
            }
            Message::ManageCalendar(command) if command.is_cancellable() => {
                self.begin_management(runtime, command, request);
            }
            Message::ManagementCompleted(command, result) => {
                let activity = runtime.sync.take().map(|sync| {
                    if result.as_ref().is_ok_and(|cancelled| !*cancelled) {
                        for entry in &sync.batch {
                            self.pushes.acknowledge(entry);
                        }
                    }
                    sync.activity
                });
                let cancelled = result.as_ref().is_ok_and(|cancelled| *cancelled);
                let completed = result.is_ok();
                let mut events = vec![Event::CalendarsLoaded(self.calendars())];
                events.extend(self.refresh_subscriptions(&self.subscriptions.ids()));
                events.push(Event::ManagementFinished(command, result.map(|_| ())));
                events.push(Event::OperationFinished {
                    completed,
                    cancelled,
                });
                let activity = if cancelled {
                    if let Some(activity) = activity {
                        let _ = runtime.results.send_blocking(Event::Activity(
                            activity.finish(Phase::Cancelled, None),
                        ));
                    }
                    None
                } else {
                    activity
                };
                publish_events(&runtime.results, events, activity, request);
                self.next_operation(runtime);
            }
            Message::ManageCalendar(command) => {
                let activity = Message::ManageCalendar(command.clone()).activity(request);
                if let Some(activity) = &activity {
                    let _ = runtime
                        .results
                        .send_blocking(Event::Activity(activity.clone()));
                }
                let result = self
                    .state
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("No loaded state"))
                    .and_then(|state| runtime.runtime.block_on(management::apply(state, &command)))
                    .map_err(|error| format!("{error:#}"));
                let mut events = vec![Event::CalendarsLoaded(self.calendars())];
                events.extend(self.refresh_subscriptions(&self.subscriptions.ids()));
                events.push(Event::ManagementFinished(command, result));
                publish_events(&runtime.results, events, activity, request);
            }
            Message::Authenticate(id, collection) => {
                self.begin_authentication(runtime, id, collection, request);
            }
            Message::CancelAuthentication(id) => runtime.cancel_authentication(id),
            Message::AuthenticationFinished(id, result) => {
                runtime.finish_authentication(id, result);
            }
            Message::SyncAll => self.begin_sync(runtime, request, None),
            Message::SyncCollection(collection) => {
                self.begin_sync(runtime, request, Some(collection));
            }
            Message::CancelSync => {
                if let Some(sync) = &runtime.sync {
                    sync.cancellation.cancel();
                }
            }
            message => {
                let finished = matches!(message, Message::SyncFinished(_));
                let activity = if finished {
                    runtime.sync.take().map(|sync| {
                        if let Message::SyncFinished(Ok(result)) = &message
                            && let Ok(state) = self.state()
                        {
                            self.pushes.acknowledge_sync(&state, &sync.batch, result);
                        }
                        sync.activity
                    })
                } else {
                    message.activity(request)
                };
                publish_events(
                    &runtime.results,
                    self.handle(message, &runtime.runtime),
                    activity,
                    request,
                );
                if finished {
                    self.next_operation(runtime);
                }
            }
        }
        if reconcile && let Ok(state) = self.state() {
            self.pushes.prune(&state);
        }
        if self.pushes.revision() != revision {
            let _ = runtime
                .results
                .send_blocking(Event::PendingPushes(self.pushes.len()));
        }
        if runtime.sync.is_none() && self.pushes.due(std::time::Instant::now()) {
            self.begin_push(runtime, request);
        }
    }

    fn begin_push(&mut self, runtime: &mut WorkerRuntime, request: u64) {
        let Some(state) = self.state.clone() else {
            return;
        };
        runtime.sync = self
            .pushes
            .begin_upload(
                state,
                &runtime.runtime,
                runtime.requests.clone(),
                runtime.results.clone(),
                request,
            )
            .map(|upload| RunningSync {
                _task: upload.task,
                cancellation: upload.cancellation,
                activity: upload.activity,
                management: true,
                batch: upload.batch,
            });
    }

    fn finish_push(
        &mut self,
        runtime: &mut WorkerRuntime,
        result: Result<eventix_state::PushResult, String>,
    ) {
        let running = runtime.sync.take();
        let (batch, activity) = running
            .map(|running| (running.batch, Some(running.activity)))
            .unwrap_or_default();
        let state = self.state().ok();
        let events = self
            .pushes
            .finish_upload(state.as_deref(), &batch, activity, result);
        drop(state);
        for event in events {
            let _ = runtime.results.send_blocking(event);
        }
    }

    fn begin_sync(
        &mut self,
        runtime: &mut WorkerRuntime,
        request: u64,
        collection: Option<String>,
    ) {
        let message = collection
            .clone()
            .map(Message::SyncCollection)
            .unwrap_or(Message::SyncAll);
        if let Some(sync) = &runtime.sync {
            if sync.management
                && !runtime.pending_operations.iter().any(|(pending, _)| {
                    match (pending, &message) {
                        (Message::SyncAll, Message::SyncAll) => true,
                        (Message::SyncCollection(a), Message::SyncCollection(b)) => a == b,
                        _ => false,
                    }
                })
            {
                runtime.pending_operations.push_back((message, request));
            }
            return;
        }
        let _ = runtime.results.send_blocking(Event::OperationStarted {
            description: "Synchronizing calendars…",
            determinate: true,
        });
        let cancellation = SyncCancellation::default();
        let activity = message.activity(request).unwrap();
        let _ = runtime
            .results
            .send_blocking(Event::Activity(activity.clone()));
        runtime.sync = self
            .start_sync(
                &runtime.runtime,
                &runtime.results,
                &runtime.requests,
                cancellation.clone(),
                &activity,
                collection,
            )
            .map(|task| RunningSync {
                _task: task,
                cancellation,
                activity,
                management: false,
                batch: self.pushes.items(),
            });
    }

    fn next_operation(&mut self, runtime: &mut WorkerRuntime) {
        while runtime.sync.is_none() {
            let Some((message, request)) = runtime.pending_operations.pop_front() else {
                break;
            };
            self.dispatch(runtime, message, request);
        }
    }

    fn begin_management(
        &mut self,
        runtime: &mut WorkerRuntime,
        command: ManagementRequest,
        request: u64,
    ) {
        let batch = self
            .state()
            .map(|state| self.pushes.management_batch(&state, &command))
            .unwrap_or_default();
        let activity = Message::ManageCalendar(command.clone())
            .activity(request)
            .unwrap();
        let _ = runtime
            .results
            .send_blocking(Event::Activity(activity.clone()));
        let _ = runtime.results.send_blocking(Event::OperationStarted {
            description: command.progress_description(),
            determinate: false,
        });
        let cancellation = SyncCancellation::default();
        let token = cancellation.clone();
        let state = self.state.clone();
        let requests = runtime.requests.clone();
        let task = runtime.runtime.spawn(async move {
            let task_command = command.clone();
            let result = tokio::spawn(async move {
                let state = state.ok_or_else(|| anyhow::anyhow!("No loaded state"))?;
                management::apply_cancellable(&state, &task_command, &token).await
            })
            .await
            .map_err(anyhow::Error::from)
            .and_then(|result| result)
            .map_err(|error| format!("{error:#}"));
            let _ = requests
                .send(Message::ManagementCompleted(command, result))
                .await;
        });
        runtime.sync = Some(RunningSync {
            _task: task,
            cancellation,
            activity,
            management: true,
            batch,
        });
    }

    fn begin_authentication(
        &self,
        runtime: &mut WorkerRuntime,
        id: u64,
        collection: String,
        request: u64,
    ) {
        if let Some(authentication) = &runtime.authentication {
            runtime.cancel_authentication(authentication.id);
        }
        let activity = Message::Authenticate(id, collection.clone())
            .activity(request)
            .unwrap();
        let _ = runtime
            .results
            .send_blocking(Event::Activity(activity.clone()));
        runtime.authentication = self
            .start_auth(
                &runtime.runtime,
                &runtime.results,
                &runtime.requests,
                id,
                collection,
                activity.clone(),
            )
            .map(|task| RunningAuthentication { id, task, activity });
    }

    fn handle(&mut self, message: Message, runtime: &tokio::runtime::Runtime) -> Vec<Event> {
        tracing::debug!("handle(message={:?})", message);
        let mut events = Vec::new();

        // load state first if not already done and the message requires it
        if self.state.is_none()
            && matches!(
                message,
                Message::ShowContent(_)
                    | Message::LoadManagement(_)
                    | Message::RefreshSidebar
                    | Message::PrepareEditor(_, _)
            )
        {
            match self.load_state() {
                Ok(event) => {
                    events.push(event);
                    if let Ok(state) = self.state() {
                        events.push(Event::PreferencesLoaded(state.settings().preferences()));
                    }
                    events.push(Event::CalendarsLoaded(self.calendars()));
                }
                Err(error) => {
                    let error = format!("{error:#}");
                    return vec![match message {
                        Message::ShowContent(request) => Event::ContentLoaded(ContentResponse {
                            generation: request.generation,
                            result: Err(error),
                        }),
                        Message::PrepareEditor(id, _) => Event::EditorLoaded(id, Err(error)),
                        Message::LoadManagement(id) => Event::ManagementLoaded(id, Err(error)),
                        _ => Event::SidebarLoaded(Err(error)),
                    }];
                }
            }
        }

        events.extend(self.handle_request(message, runtime));

        events
    }

    fn handle_request(
        &mut self,
        message: Message,
        runtime: &tokio::runtime::Runtime,
    ) -> Vec<Event> {
        let event = match message {
            Message::SavePreferences(preferences) => {
                let result = self
                    .save_preferences(preferences)
                    .map_err(|error| format!("{error:#}"));
                let mut events = vec![Event::PreferencesSaved(result)];
                if matches!(events.first(), Some(Event::PreferencesSaved(Ok(_)))) {
                    self.pushes
                        .set_delay(std::time::Duration::from_secs(u64::from(
                            preferences.post_delay_seconds,
                        )));
                    events.extend(self.refresh_subscriptions(&self.subscriptions.ids()));
                }
                return events;
            }
            Message::PushFinished(_) | Message::PushItemReady(..) => {
                unreachable!("push completion is dispatched independently")
            }
            Message::ManageCalendar(_) => unreachable!("management is dispatched with the runtime"),
            Message::LoadManagement(id) => Event::ManagementLoaded(
                id,
                self.state()
                    .map(|state| eventix_view::calendars::Snapshot::load(&state))
                    .map_err(|error| format!("{error:#}")),
            ),
            Message::ToggleCalendar(id) => return self.toggle_calendar(id),
            Message::LoadFilepath(item) => {
                Event::FilepathLoaded(self.filepath(&item).map_err(|error| format!("{error:#}")))
            }
            Message::LoadDetails(id, item, kind) => Event::DetailsLoaded(
                id,
                self.saved_item(&item, kind)
                    .map_err(|error| format!("{error:#}")),
            ),
            Message::SaveReminders(selected, command) => Event::RemindersSaved(
                self.save_reminders(selected, command, runtime)
                    .map_err(|error| format!("{error:#}")),
            ),
            Message::LoadEdit(id, item, scope) => Event::EditLoaded(
                id,
                self.load_edit(item, scope)
                    .map_err(|error| format!("{error:#}")),
            ),
            Message::SaveEdit(id, edit) => Event::EditSaved(
                id,
                self.save_edit(edit, runtime)
                    .map_err(|error| format!("{error:#}")),
            ),
            Message::PrepareEditor(id, kind) => Event::EditorLoaded(
                id,
                self.state()
                    .and_then(|state| EditorOptions::load_for(&state, kind))
                    .map_err(|error| format!("{error:#}")),
            ),
            Message::CreateItem(id, item) => Event::EditorSaved(
                id,
                self.execute(item, runtime)
                    .map_err(|error| format!("{error:#}")),
            ),
            Message::ShowContent(request) => self.handle_show_content(request),
            Message::LoadSyncCollections => self.load_sync_collections(),
            Message::LoadListRows(request) => self.load_list_rows(request),
            Message::LoadOccurrences(request) => self.load_occurrences(request),
            Message::RefreshSidebar => self.refresh_sidebar(),
            Message::RefreshSubscriptions(ids) => return self.refresh_subscriptions(&ids),
            Message::Reload => return self.reload(),
            Message::SyncAll
            | Message::SyncCollection(_)
            | Message::CancelSync
            | Message::ManagementCompleted(..) => {
                unreachable!("sync is dispatched independently of queries")
            }
            Message::Authenticate(..)
            | Message::CancelAuthentication(_)
            | Message::AuthenticationFinished(..) => {
                unreachable!("authentication is dispatched independently of queries")
            }
            Message::SyncFinished(result) => return self.sync_finished(result),
            Message::Execute(command) => match self.execute(command, runtime) {
                Ok(change) => Event::Changed(change),
                Err(error) => Event::CommandFailed(format!("{error:#}")),
            },
        };

        vec![event]
    }

    fn handle_show_content(&mut self, request: ContentRequest) -> Event {
        let result = self
            .show_content(&request)
            .map_err(|error| format!("{error:#}"));

        Event::ContentLoaded(ContentResponse {
            generation: request.generation,
            result,
        })
    }

    fn load_sync_collections(&self) -> Event {
        let collections = self
            .state()
            .map(|state| {
                state
                    .settings()
                    .collections()
                    .iter()
                    .map(|(id, collection)| {
                        use eventix_view::calendars::Backend;

                        let backend = match collection.syncer() {
                            eventix_state::SyncerType::FileSystem { .. } => Backend::FileSystem,
                            eventix_state::SyncerType::VDirSyncer { .. } => Backend::CalDav,
                            eventix_state::SyncerType::O365 { .. } => Backend::O365,
                        };
                        (id.clone(), backend)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Event::SyncCollectionsLoaded(collections)
    }

    fn load_list_rows(&self, request: ListRowsRequest) -> Event {
        Event::ListRowsLoaded(ListRowsResponse {
            revision: request.revision,
            result: self
                .state()
                .map(|state| {
                    request
                        .items
                        .into_iter()
                        .map(|item| {
                            let row = eventix_view::list::Row::load(&state, &item);
                            (item, row)
                        })
                        .collect()
                })
                .map_err(|error| format!("{error:#}")),
        })
    }

    fn load_occurrences(&self, request: OccurrencesRequest) -> Event {
        let result = self
            .state()
            .map(|state| {
                eventix_view::list::occurrences::Page::load(
                    &state,
                    &request.item,
                    &request.cursor,
                    request.direction,
                    request.count,
                )
            })
            .map_err(|error| format!("{error:#}"));
        Event::OccurrencesLoaded(OccurrencesResponse { request, result })
    }

    fn sync_finished(&mut self, result: Result<SyncResult, String>) -> Vec<Event> {
        let mut events = vec![Event::CalendarsLoaded(self.calendars())];
        if result.as_ref().map_or(true, |result| result.changed) {
            events.extend(self.refresh_subscriptions(&self.subscriptions.ids()));
        }
        events.push(Event::SyncFinished(result));
        events
    }

    fn load_state(&mut self) -> anyhow::Result<Event> {
        let state = load_local_state(self.xdg.clone())?;
        self.state = Some(Arc::new(tokio::sync::Mutex::new(state)));

        Ok(Event::StateLoaded)
    }

    fn save_preferences(
        &self,
        preferences: eventix_state::Preferences,
    ) -> anyhow::Result<eventix_state::Preferences> {
        let mut state = self.state()?;
        preferences.validate().map_err(|error| {
            anyhow::anyhow!(state.locale().translate(&error.to_string()).to_string())
        })?;
        state.save_preferences(preferences)?;
        Ok(preferences)
    }

    /// Starts network work separately so the serialized request loop remains available.
    fn start_sync(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        results: &async_channel::Sender<Event>,
        requests: &async_channel::Sender<Message>,
        cancellation: SyncCancellation,
        activity: &Activity,
        collection: Option<String>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        if self.state.is_none() {
            for event in self.handle(Message::Reload, runtime) {
                if let Event::ReloadFailed(error) = event {
                    publish_events(
                        results,
                        vec![Event::SyncFinished(Err(error))],
                        Some(activity.clone()),
                        activity.request(),
                    );
                    return None;
                }
                let _ = results.send_blocking(event);
            }
        }
        let state = self.state.clone()?;
        let progress = SessionProgress(results.clone());
        let requests = requests.clone();
        Some(runtime.spawn(async move {
            // Await a child task so panics also produce a terminal UI event.
            let result = tokio::spawn(async move {
                if let Some(collection) = collection {
                    progress.started(1);
                    let result =
                        State::sync_collection(&state, &collection, None, &cancellation).await;
                    if result.as_ref().is_ok_and(|result| !result.cancelled) {
                        progress.collection_ready(&collection, 1, 1);
                    }
                    result
                } else {
                    State::sync_all_cancellable(&state, None, Some(&progress), &cancellation).await
                }
            })
            .await
            .map_err(anyhow::Error::from)
            .and_then(|result| result)
            .map_err(|error| format!("{error:#}"));
            let _ = requests.send(Message::SyncFinished(result)).await;
        }))
    }

    fn start_auth(
        &self,
        runtime: &tokio::runtime::Runtime,
        results: &async_channel::Sender<Event>,
        requests: &async_channel::Sender<Message>,
        id: u64,
        collection: String,
        activity: Activity,
    ) -> Option<AuthenticationTask> {
        let Some(state) = self.state.clone() else {
            publish_events(
                results,
                vec![Event::AuthFinished(id, Err("No loaded state".to_string()))],
                Some(activity.clone()),
                activity.request(),
            );
            return None;
        };
        let results = results.clone();
        let requests = requests.clone();
        Some(AuthenticationTask(runtime.spawn(async move {
            let result = monitor_o365_auth(&state, &collection, |prompt| {
                let _ = results.try_send(Event::AuthPrompt(id, prompt));
            })
            .await
            .map_err(|error| format!("{error:#}"));
            let _ = requests
                .send(Message::AuthenticationFinished(id, result))
                .await;
        })))
    }

    fn reload(&mut self) -> Vec<Event> {
        let event = match self.load_state() {
            Ok(event) => event,
            Err(error) => return vec![Event::ReloadFailed(format!("{error:#}"))],
        };

        let mut events = vec![event];
        if let Ok(state) = self.state() {
            events.push(Event::PreferencesLoaded(state.settings().preferences()));
        }
        events.push(Event::CalendarsLoaded(self.calendars()));
        let ids = self.subscriptions.ids();
        events.extend(self.refresh_subscriptions(&ids));

        events
    }

    fn state(&self) -> anyhow::Result<tokio::sync::OwnedMutexGuard<State>> {
        self.state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No loaded state"))
            .map(|state| state.clone().blocking_lock_owned())
    }

    fn calendars(&self) -> Vec<CalendarVisibility> {
        self.state()
            .map(|state| calendars::load(&state))
            .unwrap_or_default()
    }

    fn toggle_calendar(&mut self, id: String) -> Vec<Event> {
        let result = (|| -> anyhow::Result<()> {
            let mut state = self.state()?;
            calendars::toggle(&mut state, &id)
        })();

        let mut events = vec![Event::CalendarsLoaded(self.calendars())];
        match result {
            Ok(()) => {
                // The list owns its calendar filter and must not inherit header visibility.
                let ids = self
                    .subscriptions
                    .ids()
                    .into_iter()
                    .filter(|id| {
                        *id == SubscriptionId::Sidebar
                            || self.content.as_ref().is_some_and(|content| {
                                content.view != View::List
                                    && *id == SubscriptionId::Calendar(content.generation)
                            })
                    })
                    .collect::<Vec<_>>();
                events.extend(self.refresh_subscriptions(&ids));
            }
            Err(error) => events.push(Event::CalendarToggleFailed(format!("{error:#}"))),
        }
        events
    }

    fn filepath(&self, item: &ItemId) -> anyhow::Result<String> {
        let state = self.state()?;
        let file = state
            .store()
            .directories()
            .iter()
            .find(|dir| dir.id().as_str() == item.calendar)
            .and_then(|dir| dir.file_by_id(&item.uid))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    state
                        .locale()
                        .translate("Occurrence is no longer available")
                        .to_string()
                )
            })?;

        Ok(std::path::absolute(file.path())?
            .to_string_lossy()
            .into_owned())
    }

    fn load_edit(
        &self,
        item: ItemId,
        scope: EditScope,
    ) -> anyhow::Result<(EditorOptions, Box<UpdateItem>)> {
        let state = self.state()?;
        let edit = UpdateItem::load(&state, item, scope)?;

        Ok((
            EditorOptions::load_for_edit(&state, edit.fields.kind, &edit.item.calendar)?,
            Box::new(edit),
        ))
    }

    fn save_edit(
        &mut self,
        edit: Box<UpdateItem>,
        runtime: &tokio::runtime::Runtime,
    ) -> anyhow::Result<(ChangeSet, SavedItem)> {
        let kind = edit.fields.kind;
        let change = self.execute(edit, runtime)?;
        let event = self.saved_item(&change.item, kind)?;

        Ok((change, event))
    }

    fn save_reminders(
        &mut self,
        selected: ItemId,
        command: UpdatePersonalAlarms,
        runtime: &tokio::runtime::Runtime,
    ) -> anyhow::Result<(ChangeSet, SavedItem)> {
        let state = self.state()?;
        let kind = state
            .store()
            .directories()
            .iter()
            .find(|dir| dir.id().as_str() == selected.calendar)
            .and_then(|dir| dir.file_by_id(&selected.uid))
            .and_then(|file| {
                file.occurrence_by_id(
                    &selected.uid,
                    selected.recurrence.as_ref(),
                    state.timezone(),
                )
            })
            .map(|occ| occ.ctype())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    state
                        .locale()
                        .translate("Occurrence is no longer available")
                        .to_string()
                )
            })?;
        drop(state);
        let change = self.execute(Box::new(command), runtime)?;
        let item = self.saved_item(&selected, kind)?;
        Ok((change, item))
    }

    fn saved_item(&self, item: &ItemId, kind: CalCompType) -> anyhow::Result<SavedItem> {
        let state = self.state()?;
        let event = match kind {
            CalCompType::Event => eventix_view::Event::load(&state, item)
                .map(|event| SavedItem::Event(Box::new(event))),
            CalCompType::Todo => eventix_view::sidebar::SidebarRow::load(&state, item)
                .map(|task| SavedItem::Task(Box::new(task))),
        }
        .ok_or_else(|| {
            anyhow::anyhow!(
                state
                    .locale()
                    .translate("Occurrence is no longer available")
                    .to_string()
            )
        })?;

        Ok(event)
    }

    fn show_content(&mut self, request: &ContentRequest) -> anyhow::Result<Snapshot> {
        let state = self.state()?;
        let content = ContentSubscription {
            generation: request.generation,
            date: request
                .date
                .unwrap_or_else(|| Utc::now().with_timezone(state.timezone()).date_naive()),
            view: request.view,
            filter: request.filter.clone(),
        };

        drop(state);
        self.load_content(content)
    }

    /// Installs a subscription only after its query and initial snapshot are available.
    fn load_content(&mut self, mut content: ContentSubscription) -> anyhow::Result<Snapshot> {
        let state = self.state()?;
        let query = content.query(&state)?;
        let snapshot = content.snapshot(&state)?;
        if let Snapshot::List(list) = &snapshot {
            content.filter = list.filter.clone();
        }

        self.subscriptions
            .register(SubscriptionId::Calendar(content.generation), query);
        self.content = Some(content);

        Ok(snapshot)
    }

    fn refresh_sidebar(&mut self) -> Event {
        let result = (|| -> anyhow::Result<SidebarSnapshot> {
            let state = self.state()?;
            let now = Utc::now().with_timezone(state.timezone());
            let snapshot = SidebarSnapshot::load(&state, now);
            self.subscriptions
                .register(SubscriptionId::Sidebar, Query::Sidebar { now });

            Ok(snapshot)
        })()
        .map_err(|error| format!("{error:#}"));

        Event::SidebarLoaded(result)
    }

    fn refresh_subscriptions(&mut self, ids: &[SubscriptionId]) -> Vec<Event> {
        let mut events = Vec::new();

        if let Some(content) = self.content.clone()
            && ids.contains(&SubscriptionId::Calendar(content.generation))
        {
            let generation = content.generation;
            let result = self
                .load_content(content)
                .map_err(|error| format!("{error:#}"));
            events.push(Event::ContentLoaded(ContentResponse { generation, result }));
        }

        if ids.contains(&SubscriptionId::Sidebar)
            && self.subscriptions.contains(SubscriptionId::Sidebar)
        {
            events.push(self.refresh_sidebar());
        }

        events
    }

    fn execute(
        &mut self,
        command: Box<dyn Action>,
        runtime: &tokio::runtime::Runtime,
    ) -> anyhow::Result<ChangeSet> {
        let state = self.state()?;

        let mut affected = Vec::new();
        let changes = ResourceChanges::capture(&state, command.affected_items());
        for item in command.affected_items() {
            for id in self.subscriptions.matching(&state, item) {
                if !affected.contains(&id) {
                    affected.push(id);
                }
            }
        }

        drop(state);
        let shared = self
            .state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No loaded state"))?;
        let item = runtime.block_on(command.execute(shared))?;
        let state = self.state()?;
        changes.observe(&mut self.pushes, &state, &item);
        for id in self.subscriptions.matching(&state, &item) {
            if !affected.contains(&id) {
                affected.push(id);
            }
        }

        Ok(ChangeSet { item, affected })
    }
}

struct SessionProgress(async_channel::Sender<Event>);

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

/// Loads an existing local profile, reporting missing settings instead of an empty calendar.
fn load_local_state(xdg: Arc<xdg::BaseDirectories>) -> anyhow::Result<eventix_state::State> {
    let state = eventix_state::State::new(xdg.clone())?;
    if xdg.find_config_file("settings.toml").is_none() {
        let expected = xdg
            .get_config_file("settings.toml")
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "settings.toml".into());
        anyhow::bail!(
            formatx::formatx!(state.locale().translate(
            "No calendar configuration found at {}. Check XDG_CONFIG_HOME and --data-prefix."
        ), expected)
            .unwrap()
        );
    }

    Ok(state)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use chrono::{Duration, NaiveDate, TimeZone};
    use eventix_ical::col::CalDir;
    use eventix_state::actions::items::MoveEvent;
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};

    use super::{
        ContentRequest, Event, ItemId, Message, Query, Snapshot, SubscriptionId, View, Worker,
        load_local_state,
    };

    fn test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn worker(path: &Path) -> Worker {
        fs::write(
            path.join("event.ics"),
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
            UID:move-me\r\nDTSTART:20261012T090000Z\r\nDTEND:20261012T100000Z\r\n\
            RRULE:FREQ=DAILY;COUNT=5\r\nSUMMARY:Move me\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(path, path));
        let mut state = State::new_with_timezone(xdg.clone(), Some(chrono_tz::UTC)).unwrap();
        state.store_mut().add(
            CalDir::new_from_dir(Arc::new("test".into()), path.into(), "Test".into(), false)
                .unwrap(),
        );
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: path.display().to_string(),
        });
        let mut settings = CalendarSettings::default();
        settings.set_enabled(true);
        collection
            .all_calendars_mut()
            .insert("test".into(), settings);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        let mut worker = Worker::new(xdg);
        worker.state = Some(Arc::new(tokio::sync::Mutex::new(state)));
        worker
    }

    #[test]
    fn sync_results_publish_calendar_errors_and_keep_them_across_visibility_refreshes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker
            .state()
            .unwrap()
            .misc_mut()
            .set_collection_error("local", Some("Connection refused"));
        let events = worker.sync_finished(Ok(eventix_state::SyncResult {
            collections: [(
                "local".into(),
                eventix_state::SyncColResult::Error("Connection refused".into()),
            )]
            .into(),
            ..Default::default()
        }));
        let Event::CalendarsLoaded(calendars) = &events[0] else {
            panic!("expected header calendars");
        };
        assert_eq!(
            calendars[0].sync_error.as_deref(),
            Some("Connection refused")
        );
        assert_eq!(
            worker.calendars()[0].sync_error.as_deref(),
            Some("Connection refused")
        );

        worker
            .state()
            .unwrap()
            .misc_mut()
            .set_collection_error("local", None);
        worker.sync_finished(Ok(eventix_state::SyncResult {
            collections: [("local".into(), eventix_state::SyncColResult::Success(false))].into(),
            ..Default::default()
        }));
        assert_eq!(worker.calendars()[0].sync_error, None);
    }

    #[test]
    fn language_change_refreshes_content_and_sidebar_and_persists_without_changing_timezone() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker.handle(
            Message::ShowContent(ContentRequest {
                generation: 42,
                date: Some("2026-10-12".parse().unwrap()),
                view: View::Week,
                filter: Default::default(),
            }),
            &test_runtime(),
        );
        worker.handle(Message::RefreshSidebar, &test_runtime());
        for language in [
            eventix_locale::LocaleType::German,
            eventix_locale::LocaleType::English,
        ] {
            let preferences = eventix_state::Preferences {
                locale_type: language,
                sync_interval_minutes: 30,
                post_delay_seconds: 12,
            };
            let events = worker.handle(Message::SavePreferences(preferences), &test_runtime());
            assert!(
                matches!(events.first(), Some(Event::PreferencesSaved(Ok(saved))) if *saved == preferences)
            );
            assert!(events.iter().any(|event| matches!(event,
                Event::ContentLoaded(response) if response.generation == 42
                    && matches!(&response.result, Ok(Snapshot::Week(week))
                        if week.locale.ty() == language))));
            assert!(events.iter().any(|event| matches!(event,
                Event::SidebarLoaded(Ok(sidebar)) if sidebar.locale.ty() == language)));
            let state = worker.state().unwrap();
            assert_eq!(state.locale().ty(), language);
            assert_eq!(*state.locale().timezone(), chrono_tz::UTC);
            let reloaded =
                State::new_with_timezone(worker.xdg.clone(), Some(chrono_tz::UTC)).unwrap();
            assert_eq!(reloaded.locale().ty(), language);
            assert_eq!(reloaded.settings().preferences(), preferences);
        }
    }

    #[test]
    fn settings_failed_persistence_restores_all_preferences_and_the_loaded_locale() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        let previous = worker.state().unwrap().settings().preferences();
        let locale = worker.state().unwrap().locale();
        // A directory at the destination prevents the atomic file replacement.
        fs::create_dir(tmp.path().join("settings.toml")).unwrap();
        let preferences = eventix_state::Preferences {
            locale_type: eventix_locale::LocaleType::German,
            sync_interval_minutes: 60,
            post_delay_seconds: 0,
        };
        let events = worker.handle(Message::SavePreferences(preferences), &test_runtime());
        assert!(matches!(
            events.as_slice(),
            [Event::PreferencesSaved(Err(_))]
        ));
        let state = worker.state().unwrap();
        assert_eq!(state.settings().preferences(), previous);
        assert!(Arc::ptr_eq(&state.locale(), &locale));
    }

    #[test]
    fn management_requests_return_current_owned_data_without_replacing_content_queries() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker.handle(
            Message::ShowContent(ContentRequest {
                generation: 42,
                date: Some("2026-10-12".parse().unwrap()),
                view: View::Week,
                filter: Default::default(),
            }),
            &test_runtime(),
        );
        let subscriptions = worker.subscriptions.ids();
        let events = worker.handle(Message::LoadManagement(7), &test_runtime());
        let [Event::ManagementLoaded(7, Ok(snapshot))] = events.as_slice() else {
            panic!("expected a management snapshot");
        };
        assert_eq!(snapshot.collections[0].id, "local");
        assert_eq!(snapshot.collections[0].calendars[0].id, "test");
        assert!(snapshot.collections[0].calendars[0].enabled);
        assert_eq!(worker.subscriptions.ids(), subscriptions);
        assert_eq!(worker.content.as_ref().unwrap().generation, 42);
        worker
            .state()
            .unwrap()
            .settings_mut()
            .collections_mut()
            .clear();
        let events = worker.handle(Message::LoadManagement(8), &test_runtime());
        let [Event::ManagementLoaded(8, Ok(snapshot))] = events.as_slice() else {
            panic!("expected refreshed management data");
        };
        assert!(snapshot.collections.is_empty());
    }

    #[test]
    fn management_mutations_refresh_subscriptions_and_publish_scoped_results() {
        use crate::model::management::{CalendarOperation, Operation, Request};

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker.handle(
            Message::ShowContent(ContentRequest {
                generation: 42,
                date: Some("2026-10-12".parse().unwrap()),
                view: View::List,
                filter: Default::default(),
            }),
            &test_runtime(),
        );
        let (requests, _) = async_channel::unbounded();
        let (results, responses) = async_channel::unbounded();
        let mut runtime = super::WorkerRuntime::new(requests, results).unwrap();
        worker.dispatch(
            &mut runtime,
            Message::ManageCalendar(Request {
                collection: "local".into(),
                calendar: "test".into(),
                folder: String::new(),
                configured: true,
                operation: Operation::Calendar(CalendarOperation::Enable(false)),
            }),
            1,
        );
        let mut events = Vec::new();
        while let Ok(event) = responses.try_recv() {
            events.push(event);
        }
        assert!(events.iter().any(|event| matches!(event, Event::ManagementFinished(request, Ok(())) if request.calendar == "test")));
        assert!(events.iter().any(
            |event| matches!(event, Event::CalendarsLoaded(calendars) if calendars.is_empty())
        ));
        assert!(events.iter().any(|event| matches!(event, Event::ContentLoaded(response) if response.generation == 42 && response.result.is_ok())));
    }

    #[test]
    fn activity_records_mutations_new_uids_and_read_errors_but_not_successful_reads() {
        use super::CalendarSession;
        use crate::model::activity::Phase;
        use eventix_state::actions::items::{DeleteItem, EditScope, UpdateItem};

        let tmp = tempfile::tempdir().unwrap();
        let worker = worker(tmp.path());
        let item = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: None,
        };
        let fields = {
            let mut state = worker.state().unwrap();
            state
                .settings_mut()
                .collections_mut()
                .get_mut("local")
                .unwrap()
                .all_calendars_mut()
                .get_mut("test")
                .unwrap()
                .set_types(vec![eventix_ical::objects::CalCompType::Event]);
            UpdateItem::load(&state, item.clone(), EditScope::Series)
                .unwrap()
                .fields
        };
        let mut missing = item.clone();
        missing.uid = "missing".into();
        let (session, responses) = CalendarSession::start_worker(worker);
        session.load_details(91, item, eventix_ical::objects::CalCompType::Event);
        session.create_item(92, Box::new(fields));
        session.execute(Box::new(DeleteItem {
            item: missing.clone(),
        }));
        // Even a failed read superseded in the UI must remain in the history.
        session.load_details(90, missing, eventix_ical::objects::CalCompType::Event);
        session.refresh_sidebar();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (entries, created) = runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut entries = Vec::new();
                let mut created = None;
                loop {
                    match responses.recv().await.unwrap() {
                        Event::Activity(entry) => entries.push(entry),
                        Event::EditorSaved(92, Ok(change)) => created = Some(change.item),
                        Event::SidebarLoaded(Ok(_)) => break,
                        _ => {}
                    }
                }
                (entries, created.unwrap())
            })
            .await
            .unwrap()
        });
        assert_eq!(entries.len(), 3);
        let xdg = xdg::BaseDirectories::with_prefix(tmp.path().file_name().unwrap());
        let locale = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::English,
            chrono_tz::UTC,
        )
        .unwrap();
        assert_eq!(entries[0].summary(locale.as_ref()), "New event created");
        assert_eq!(entries[0].phase(), Phase::Completed);
        assert_eq!(entries[0].items(), std::slice::from_ref(&created));
        assert_ne!(created.uid, "move-me");
        assert_eq!(entries[1].summary(locale.as_ref()), "Item deletion failed");
        assert_eq!(entries[1].phase(), Phase::Failed);
        assert_eq!(entries[1].items()[0].uid, "missing");
        assert_eq!(entries[2].summary(locale.as_ref()), "Item loading failed");
        assert_eq!(entries[2].phase(), Phase::Failed);
    }

    #[test]
    fn sync_activity_keeps_partial_failures_and_cancellation_without_claiming_success() {
        use super::publish_events;
        use crate::model::activity::{Activity, Phase};

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
                Event::CalendarsLoaded(vec![super::CalendarVisibility {
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

    #[test]
    fn management_and_full_sync_share_one_background_operation_queue() {
        use crate::model::management::{CollectionOperation, Operation, Request};

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        let (requests, receiver) = async_channel::unbounded();
        let (results, responses) = async_channel::unbounded();
        let mut runtime = super::WorkerRuntime::new(requests, results).unwrap();
        let command = Request {
            collection: "local".into(),
            calendar: "test".into(),
            folder: String::new(),
            configured: true,
            operation: Operation::Collection(CollectionOperation::Sync),
        };
        worker.dispatch(&mut runtime, Message::ManageCalendar(command.clone()), 1);
        worker.dispatch(&mut runtime, Message::ManageCalendar(command), 2);
        worker.dispatch(&mut runtime, Message::SyncAll, 3);
        worker.dispatch(&mut runtime, Message::SyncAll, 4);
        assert_eq!(runtime.pending_operations.len(), 2);
        for request in 5..8 {
            let message = runtime.runtime.block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
                    .await
                    .unwrap()
                    .unwrap()
            });
            worker.dispatch(&mut runtime, message, request);
        }
        assert!(runtime.sync.is_none());
        assert!(runtime.pending_operations.is_empty());
        let mut lifecycle = Vec::new();
        while let Ok(event) = responses.try_recv() {
            match event {
                Event::OperationStarted { determinate, .. } => lifecycle.push((true, determinate)),
                Event::OperationFinished { .. } => lifecycle.push((false, false)),
                Event::SyncFinished(_) => lifecycle.push((false, true)),
                _ => {}
            }
        }
        assert_eq!(
            lifecycle,
            [
                (true, false),
                (false, false),
                (true, false),
                (false, false),
                (true, true),
                (false, true),
            ]
        );
    }

    #[test]
    fn management_background_operations_can_be_cancelled_without_blocking_queries() {
        use crate::model::management::{
            CalendarOperation, CollectionOperation, Operation, Request,
        };

        for operation in [
            Operation::Calendar(CalendarOperation::Reload),
            Operation::Collection(CollectionOperation::Reload),
            Operation::Collection(CollectionOperation::Discover),
            Operation::Collection(CollectionOperation::Sync),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            fs::create_dir(tmp.path().join("vdirsyncer")).unwrap();
            let mut worker = worker(tmp.path());
            subscribe(&mut worker, 17, "2026-10-12", View::Week);
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let listener = runtime
                .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
                .unwrap();
            let address = listener.local_addr().unwrap();
            worker
                .state()
                .unwrap()
                .settings_mut()
                .collections_mut()
                .get_mut("local")
                .unwrap()
                .set_syncer(SyncerType::VDirSyncer {
                    email: eventix_state::EmailAccount::new(
                        "Test".into(),
                        "test@example.com".into(),
                    ),
                    url: format!("http://{address}/"),
                    read_only: false,
                    username: None,
                    password: None,
                    time_span: Default::default(),
                });
            let (session, responses) = super::CalendarSession::start_worker(worker);
            session.manage_calendar(Request {
                collection: "local".into(),
                calendar: "test".into(),
                folder: String::new(),
                configured: true,
                operation,
            });
            runtime.block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(15), async {
                    let (_connection, _) = listener.accept().await.unwrap();
                    session.load_management(42);
                    let mut started = false;
                    loop {
                        match responses.recv().await.unwrap() {
                            Event::OperationStarted { determinate, .. } => {
                                assert!(!determinate);
                                started = true;
                            }
                            Event::ManagementLoaded(42, result) => {
                                assert!(result.is_ok());
                                break;
                            }
                            _ => {}
                        }
                    }
                    assert!(started);
                    session.cancel_sync();
                    session.cancel_sync();
                    let mut refreshed = false;
                    let mut cancelled_activity = false;
                    let mut finished = false;
                    loop {
                        match responses.recv().await.unwrap() {
                            Event::ContentLoaded(response) => {
                                assert_eq!(response.generation, 17);
                                assert!(response.result.is_ok());
                                refreshed = true;
                            }
                            Event::Activity(activity) => {
                                assert_ne!(activity.phase(), super::Phase::Completed);
                                cancelled_activity |= activity.phase() == super::Phase::Cancelled;
                            }
                            Event::ManagementFinished(_, result) => {
                                assert!(result.is_ok());
                                finished = true;
                            }
                            Event::OperationFinished {
                                completed,
                                cancelled,
                            } => {
                                assert!(completed && cancelled);
                                assert!(refreshed && cancelled_activity && finished);
                                break;
                            }
                            _ => {}
                        }
                    }
                })
                .await
                .unwrap();
            });
        }
    }

    #[test]
    fn background_sync_cancellation_finishes_once_refreshes_content_and_allows_another_sync() {
        use super::CalendarSession;

        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("vdirsyncer")).unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 17, "2026-10-12", View::Week);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let address = listener.local_addr().unwrap();
        let shared = worker.state.clone().unwrap();
        {
            let mut state = shared.blocking_lock();
            state
                .settings_mut()
                .collections_mut()
                .get_mut("local")
                .unwrap()
                .set_syncer(SyncerType::VDirSyncer {
                    email: eventix_state::EmailAccount::new(
                        "Test".into(),
                        "test@example.com".into(),
                    ),
                    url: format!("http://{address}/"),
                    read_only: false,
                    username: None,
                    password: None,
                    time_span: Default::default(),
                });
        }
        let (session, responses) = CalendarSession::start_worker(worker);
        session.sync_all();
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                let (_connection, _) = listener.accept().await.unwrap();
                session.sync_all();
                session.cancel_sync();
                session.cancel_sync();
                let mut refreshed = false;
                loop {
                    match responses.recv().await.unwrap() {
                        Event::ContentLoaded(response) => {
                            assert_eq!(response.generation, 17);
                            assert!(response.result.is_ok());
                            refreshed = true;
                        }
                        Event::SyncFinished(result) => {
                            assert!(result.unwrap().cancelled);
                            assert!(refreshed);
                            break;
                        }
                        _ => {}
                    }
                }
                session.cancel_sync();
                session.refresh_sidebar();
                loop {
                    match responses.recv().await.unwrap() {
                        Event::SidebarLoaded(_) => break,
                        Event::SyncFinished(_) | Event::SyncProgress { .. } => {
                            panic!("duplicate sync")
                        }
                        _ => {}
                    }
                }
                shared
                    .lock()
                    .await
                    .settings_mut()
                    .collections_mut()
                    .get_mut("local")
                    .unwrap()
                    .set_syncer(SyncerType::FileSystem {
                        path: tmp.path().display().to_string(),
                    });
                session.sync_all();
                loop {
                    if let Event::SyncFinished(result) = responses.recv().await.unwrap() {
                        assert!(!result.unwrap().cancelled);
                        break;
                    }
                }
            })
            .await
            .unwrap();
        });
    }

    #[test]
    fn collection_sync_lists_all_collections_but_only_syncs_the_selected_one() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("vdirsyncer")).unwrap();
        let mut worker = worker(tmp.path());
        worker
            .state()
            .unwrap()
            .settings_mut()
            .collections_mut()
            .insert(
                "unrelated".into(),
                CollectionSettings::new(SyncerType::FileSystem {
                    path: tmp.path().join("missing").display().to_string(),
                }),
            );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(matches!(
            worker.handle(Message::LoadSyncCollections, &runtime).as_slice(),
            [Event::SyncCollectionsLoaded(collections)] if collections == &[
                ("local".into(), eventix_view::calendars::Backend::FileSystem),
                ("unrelated".into(), eventix_view::calendars::Backend::FileSystem),
            ]
        ));
        let (session, responses) = super::CalendarSession::start_worker(worker);
        assert!(session.sync_collection("local".into()));
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut progress = Vec::new();
                loop {
                    match responses.recv().await.unwrap() {
                        Event::SyncProgress { completed, total } => {
                            progress.push((completed, total))
                        }
                        Event::SyncFinished(result) => {
                            let result = result.unwrap();
                            assert!(!result.cancelled);
                            assert_eq!(
                                result.collections.keys().collect::<Vec<_>>(),
                                vec!["local"]
                            );
                            assert_eq!(progress, [(0, 1), (1, 1)]);
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .await
            .unwrap();
        });
    }

    #[test]
    fn background_sync_streams_progress_refreshes_subscriptions_and_suppresses_duplicates() {
        use super::CalendarSession;

        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("vdirsyncer")).unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 17, "2026-10-12", View::Week);
        fs::write(
            tmp.path().join("new.ics"),
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
             UID:synced\r\nDTSTART:20261012T110000Z\r\nDTEND:20261012T120000Z\r\n\
             SUMMARY:Synced event\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let shared = worker.state.clone().unwrap();
        let guard = shared.blocking_lock();
        let (session, responses) = CalendarSession::start_worker(worker);
        session.sync_all();
        session.sync_all();
        session.refresh_sidebar();
        drop(guard);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut progress = Vec::new();
                let mut activities = Vec::new();
                let mut refreshed = false;
                loop {
                    match responses.recv().await.unwrap() {
                        Event::Activity(entry) => activities.push(entry),
                        Event::SyncProgress { completed, total } => {
                            progress.push((completed, total))
                        }
                        Event::ContentLoaded(response) => {
                            assert_eq!(response.generation, 17);
                            let Snapshot::Week(week) = response.result.unwrap() else {
                                panic!("expected the selected week");
                            };
                            assert_eq!(week.start.to_string(), "2026-10-12");
                            assert!(
                                week.days[0]
                                    .timed
                                    .iter()
                                    .any(|event| event.event.id.uid == "synced")
                            );
                            refreshed = true;
                        }
                        Event::SyncFinished(result) => {
                            assert!(result.unwrap().changed);
                            assert!(refreshed);
                            assert_eq!(progress, [(0, 1), (1, 1)]);
                            assert_eq!(activities.len(), 2);
                            assert_eq!(activities[0].phase(), super::Phase::Started);
                            assert_eq!(activities[1].phase(), super::Phase::Completed);
                            assert_eq!(activities[0].request(), activities[1].request());
                            break;
                        }
                        _ => {}
                    }
                }
                // A request queued after completion is a fence for any duplicate work.
                session.refresh_sidebar();
                while let Ok(event) = responses.recv().await {
                    match event {
                        Event::SidebarLoaded(_) => break,
                        Event::SyncProgress { .. } | Event::SyncFinished(_) => {
                            panic!("duplicate sync")
                        }
                        _ => {}
                    }
                }
            })
            .await
            .unwrap();
        });
    }

    #[test]
    fn filepath_requests_preserve_calendar_identity_and_resolve_occurrences() {
        let tmp = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        let other_path = other.path().join("different filename.ics");
        fs::copy(tmp.path().join("event.ics"), &other_path).unwrap();
        worker.state().unwrap().store_mut().add(
            CalDir::new_from_dir(
                Arc::new("other".into()),
                other.path().into(),
                "Other".into(),
                true,
            )
            .unwrap(),
        );

        for (calendar, path) in [
            ("test", tmp.path().join("event.ics")),
            ("other", other_path),
        ] {
            for recurrence in [None, Some("TU2026-10-13T09:00:00".parse().unwrap())] {
                let events = worker.handle(
                    Message::LoadFilepath(ItemId {
                        calendar: calendar.into(),
                        uid: "move-me".into(),
                        recurrence,
                    }),
                    &test_runtime(),
                );
                let [Event::FilepathLoaded(Ok(actual))] = events.as_slice() else {
                    panic!("expected the requested filepath");
                };
                assert_eq!(actual, &path.to_string_lossy());
            }
        }
        for (calendar, uid) in [("missing", "move-me"), ("test", "missing")] {
            assert!(matches!(
                worker
                    .handle(
                        Message::LoadFilepath(ItemId {
                            calendar: calendar.into(),
                            uid: uid.into(),
                            recurrence: None,
                        }),
                        &test_runtime()
                    )
                    .as_slice(),
                [Event::FilepathLoaded(Err(_))]
            ));
        }
    }

    fn command(worker: &Worker, start: &str) -> MoveEvent {
        let item = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: Some("TU2026-10-13T09:00:00".parse().unwrap()),
        };
        let state = worker.state().unwrap();
        let occurrence = state
            .store()
            .occurrence_by_id(&item.uid, item.recurrence.as_ref(), state.timezone())
            .unwrap();
        MoveEvent {
            item,
            start: start.parse().unwrap(),
            expected_start: occurrence.occurrence_start().unwrap().to_utc(),
            expected_end: occurrence.occurrence_end().unwrap().to_utc(),
        }
    }

    #[test]
    fn persisted_moves_and_series_splits_collect_all_changed_remote_resources() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker
            .state()
            .unwrap()
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .set_syncer(SyncerType::VDirSyncer {
                email: eventix_state::EmailAccount::new("Test".into(), "test@example.com".into()),
                url: "http://localhost/".into(),
                username: None,
                password: None,
                read_only: false,
                time_span: Default::default(),
            });
        let request = command(&worker, "2026-10-13T14:30:00");
        let stale = command(&worker, "2026-10-13T15:30:00");
        worker.execute(Box::new(request), &test_runtime()).unwrap();
        assert_eq!(worker.pushes.items().len(), 1);
        assert!(worker.pushes.items()[0].item.recurrence.is_none());
        let revision = worker.pushes.revision();
        assert!(worker.execute(Box::new(stale), &test_runtime()).is_err());
        assert_eq!(worker.pushes.revision(), revision);
        let item = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: Some("TU2026-10-14T09:00:00".parse().unwrap()),
        };
        let mut edit = eventix_state::actions::items::UpdateItem::load(
            &worker.state().unwrap(),
            item,
            eventix_state::actions::items::EditScope::Following,
        )
        .unwrap();
        edit.fields.summary = "Following events".into();
        worker.execute(Box::new(edit), &test_runtime()).unwrap();
        let items = worker.pushes.items();
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|entry| entry.item.uid == "move-me"));
        assert!(items.iter().any(|entry| entry.item.uid != "move-me"));
        assert!(items.iter().all(|entry| entry.generation > revision));
    }

    #[test]
    fn undated_task_creation_and_completion_refresh_sidebar_and_return_task_details() {
        use eventix_ical::objects::{CalCompType, CalTodoStatus};
        use eventix_state::actions::items::{EditScope, TaskStatus, UpdateItem};

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker
            .state()
            .unwrap()
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .all_calendars_mut()
            .get_mut("test")
            .unwrap()
            .set_types(vec![CalCompType::Event, CalCompType::Todo]);
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let Some(Event::EditorLoaded(1, Ok(options))) = worker
            .handle(
                Message::PrepareEditor(1, CalCompType::Todo),
                &test_runtime(),
            )
            .pop()
        else {
            panic!("expected task editor options");
        };
        assert_eq!(options.kind, CalCompType::Todo);
        assert_eq!(options.calendars.len(), 1);
        let source = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: None,
        };
        let mut fields = UpdateItem::load(&worker.state().unwrap(), source, EditScope::Series)
            .unwrap()
            .fields;
        fields.kind = CalCompType::Todo;
        fields.summary = "Undated task".into();
        fields.start = None;
        fields.end = None;
        fields.recurrence = None;
        fields.task_status = Some(TaskStatus {
            status: CalTodoStatus::NeedsAction,
            percent: None,
            completed: None,
        });
        let Some(Event::EditorSaved(2, Ok(change))) = worker
            .handle(Message::CreateItem(2, Box::new(fields)), &test_runtime())
            .pop()
        else {
            panic!("expected task creation");
        };
        assert_eq!(change.affected, vec![SubscriptionId::Sidebar]);
        let item = change.item;
        let Some(Event::EditLoaded(3, Ok((options, mut edit)))) = worker
            .handle(
                Message::LoadEdit(3, item, EditScope::Series),
                &test_runtime(),
            )
            .pop()
        else {
            panic!("expected task edit");
        };
        assert_eq!(options.kind, CalCompType::Todo);
        edit.fields.task_status.as_mut().unwrap().status = CalTodoStatus::Completed;
        let Some(Event::EditSaved(3, Ok((change, super::SavedItem::Task(task))))) = worker
            .handle(Message::SaveEdit(3, edit), &test_runtime())
            .pop()
        else {
            panic!("expected saved task details");
        };
        assert_eq!(task.summary, "Undated task");
        assert!(task.start.is_none() && task.due.is_none());
        assert_eq!(change.affected, vec![SubscriptionId::Sidebar]);
        let events = worker.handle(
            Message::RefreshSubscriptions(change.affected),
            &test_runtime(),
        );
        assert!(
            matches!(events.as_slice(), [Event::SidebarLoaded(Ok(snapshot))] if snapshot.tasks.is_empty())
        );
    }

    #[test]
    fn editor_requests_preserve_identity_and_creation_invalidates_matching_queries() {
        use eventix_ical::objects::{CalCompType, CalDate, CalDateType};
        use eventix_state::actions::items::CreateItem;

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker
            .state()
            .unwrap()
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .all_calendars_mut()
            .get_mut("test")
            .unwrap()
            .set_types(vec![CalCompType::Event]);
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let Some(Event::EditorLoaded(42, Ok(options))) = worker
            .handle(
                Message::PrepareEditor(42, CalCompType::Event),
                &test_runtime(),
            )
            .pop()
        else {
            panic!("editor options should echo the request identity");
        };
        assert_eq!(options.calendars.len(), 1);
        assert_eq!(options.calendars[0].id, "test");
        let make_request = || {
            Box::new(CreateItem {
                kind: CalCompType::Event,
                task_status: None,
                calendar: "test".into(),
                summary: "Created".into(),
                location: String::new(),
                description: String::new(),
                start: Some(CalDate::new_date(
                    "2026-10-14".parse().unwrap(),
                    CalDateType::Inclusive,
                )),
                end: Some(CalDate::new_date(
                    "2026-10-15".parse().unwrap(),
                    CalDateType::Exclusive,
                )),
                recurrence: None,
                alarms: Vec::new(),
                personal_alarms: None,
                attendees: Vec::new(),
            })
        };
        let mut invalid = make_request();
        invalid.summary.clear();
        assert!(matches!(
            worker
                .handle(Message::CreateItem(43, invalid), &test_runtime())
                .as_slice(),
            [Event::EditorSaved(43, Err(_))]
        ));
        let Some(Event::EditorSaved(44, Ok(change))) = worker
            .handle(Message::CreateItem(44, make_request()), &test_runtime())
            .pop()
        else {
            panic!("successful save must publish the request identity and committed changes");
        };
        assert_eq!(change.item.calendar, "test");
        assert!(change.affected.contains(&SubscriptionId::Calendar(1)));
        assert!(change.affected.contains(&SubscriptionId::Sidebar));
        assert!(
            matches!(worker.handle(Message::RefreshSubscriptions(change.affected), &test_runtime()).as_slice(),
            [Event::ContentLoaded(response), Event::SidebarLoaded(Ok(_))] if response.result.is_ok())
        );
    }

    fn subscribe(worker: &mut Worker, generation: u64, date: &str, view: View) {
        let response = worker.show_content(&ContentRequest {
            generation,
            date: Some(date.parse().unwrap()),
            view,
            filter: Default::default(),
        });
        assert!(response.is_ok());
        let now = chrono_tz::UTC
            .with_ymd_and_hms(2026, 10, 12, 0, 0, 0)
            .unwrap();
        worker
            .subscriptions
            .register(SubscriptionId::Sidebar, Query::Sidebar { now });
    }

    #[test]
    fn edit_requests_include_current_calendar_without_creation_support() {
        use eventix_ical::objects::CalCompType;
        use eventix_state::actions::items::EditScope;

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        let mut state = worker.state().unwrap();
        let collection = state
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap();
        collection
            .all_calendars_mut()
            .get_mut("test")
            .unwrap()
            .set_types(vec![CalCompType::Todo]);
        for (id, kind) in [("events", CalCompType::Event), ("tasks", CalCompType::Todo)] {
            let mut settings = CalendarSettings::default();
            settings.set_enabled(true);
            settings.set_types(vec![kind]);
            settings.set_name(id.into());
            collection.all_calendars_mut().insert(id.into(), settings);
        }

        drop(state);
        let Some(Event::EditorLoaded(1, Ok(options))) = worker
            .handle(
                Message::PrepareEditor(1, CalCompType::Event),
                &test_runtime(),
            )
            .pop()
        else {
            panic!("expected creation options");
        };
        assert_eq!(options.calendars.len(), 1);
        assert_eq!(options.calendars[0].id, "events");

        let item = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: None,
        };
        let Some(Event::EditLoaded(2, Ok((options, _)))) = worker
            .handle(
                Message::LoadEdit(2, item, EditScope::Series),
                &test_runtime(),
            )
            .pop()
        else {
            panic!("expected edit options");
        };
        let ids: Vec<_> = options
            .calendars
            .iter()
            .map(|cal| cal.id.as_str())
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"test"));
        assert!(ids.contains(&"events"));
    }

    #[test]
    fn edit_requests_return_saved_details_and_invalidate_old_and_new_query_membership() {
        use eventix_state::actions::items::EditScope;

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 8, "2026-10-12", View::Week);
        let item = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: Some("TU2026-10-13T09:00:00".parse().unwrap()),
        };
        let Some(Event::EditLoaded(42, Ok((_, mut edit)))) = worker
            .handle(
                Message::LoadEdit(42, item.clone(), EditScope::Occurrence),
                &test_runtime(),
            )
            .pop()
        else {
            panic!("expected edit draft");
        };
        assert_eq!(edit.fields.summary, "Move me");
        let stale = edit.clone();
        edit.fields.summary = "Edited occurrence".into();
        edit.fields.start = Some("TU2026-11-10T09:00:00".parse().unwrap());
        edit.fields.end = Some("TU2026-11-10T10:00:00".parse().unwrap());
        let Some(Event::EditSaved(42, Ok((change, super::SavedItem::Event(event))))) = worker
            .handle(Message::SaveEdit(42, edit), &test_runtime())
            .pop()
        else {
            panic!("expected saved details");
        };
        assert_eq!(change.item, item);
        assert!(change.affected.contains(&SubscriptionId::Calendar(8)));
        assert_eq!(event.summary, "Edited occurrence");
        assert_eq!(event.start.date_naive().to_string(), "2026-11-10");
        assert!(matches!(
            worker
                .handle(Message::SaveEdit(43, stale), &test_runtime())
                .as_slice(),
            [Event::EditSaved(43, Err(_))]
        ));
        let responses = worker.handle(
            Message::RefreshSubscriptions(change.affected),
            &test_runtime(),
        );
        let Some(Event::ContentLoaded(response)) = responses.first() else {
            panic!("expected calendar refresh");
        };
        let Ok(Snapshot::Week(week)) = &response.result else {
            panic!("expected week");
        };
        assert!(week.days[1].timed.is_empty());
    }

    #[test]
    fn following_edits_invalidate_source_and_destination_queries_with_one_returned_identity() {
        use eventix_state::actions::items::{EditScope, UpdateItem};

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 10, "2026-11-09", View::Week);
        let item = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: Some("TU2026-10-13T09:00:00".parse().unwrap()),
        };
        let mut edit =
            UpdateItem::load(&worker.state().unwrap(), item.clone(), EditScope::Following).unwrap();
        edit.fields.start = Some("TU2026-11-10T09:00:00".parse().unwrap());
        edit.fields.end = Some("TU2026-11-10T10:00:00".parse().unwrap());

        let change = worker.execute(Box::new(edit), &test_runtime()).unwrap();
        assert_ne!(change.item.uid, item.uid);
        assert!(change.item.recurrence.is_none());
        assert!(change.affected.contains(&SubscriptionId::Sidebar));
        assert!(change.affected.contains(&SubscriptionId::Calendar(10)));

        let state = worker.state().unwrap();
        let old_file = state.store().file_by_id(&item.uid).unwrap();
        let start = chrono_tz::UTC
            .with_ymd_and_hms(2026, 10, 12, 0, 0, 0)
            .unwrap();
        assert_eq!(
            old_file
                .occurrences_between(start, start + Duration::days(7), |_| true)
                .count(),
            1
        );
        assert!(state.store().file_by_id(&change.item.uid).is_some());
    }

    #[test]
    fn series_edits_refresh_periods_containing_later_occurrences() {
        use eventix_ical::objects::{EventLike, UpdatableEventLike};
        use eventix_state::actions::items::EditScope;

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker
            .state()
            .unwrap()
            .store_mut()
            .file_by_id_mut("move-me")
            .unwrap()
            .component_with_mut(|component| component.rid().is_none())
            .unwrap()
            .set_rrule(Some("FREQ=DAILY;COUNT=20".parse().unwrap()));
        subscribe(&mut worker, 9, "2026-10-19", View::Week);
        let item = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: None,
        };
        let Some(Event::EditLoaded(1, Ok((_, mut edit)))) = worker
            .handle(
                Message::LoadEdit(1, item, EditScope::Series),
                &test_runtime(),
            )
            .pop()
        else {
            panic!("expected series draft");
        };
        edit.fields.summary = "Edited series".into();
        let Some(Event::EditSaved(1, Ok((change, _)))) = worker
            .handle(Message::SaveEdit(1, edit), &test_runtime())
            .pop()
        else {
            panic!("expected saved series");
        };
        assert!(change.affected.contains(&SubscriptionId::Calendar(9)));
    }

    #[test]
    fn moves_invalidate_old_and_new_query_membership_and_refetch_only_affected_views() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let request = command(&worker, "2026-11-10T14:30:00");
        let Some(Event::Changed(change)) = worker
            .handle(Message::Execute(Box::new(request)), &test_runtime())
            .pop()
        else {
            panic!("expected committed identity notification")
        };
        assert_eq!(change.item.uid, "move-me");
        assert!(
            change.affected.contains(&SubscriptionId::Calendar(1)),
            "old range is invalidated"
        );
        assert!(
            change.affected.contains(&SubscriptionId::Sidebar),
            "old sidebar membership is invalidated"
        );
        let mut events = worker
            .handle(
                Message::RefreshSubscriptions(change.affected),
                &test_runtime(),
            )
            .into_iter();
        let Some(Event::ContentLoaded(response)) = events.next() else {
            panic!("expected query refresh")
        };
        let Snapshot::Week(week) = response.result.unwrap() else {
            panic!("expected week")
        };
        assert_eq!(
            week.days[1].timed.len(),
            0,
            "moved exception left the old day"
        );
        assert!(matches!(events.next(), Some(Event::SidebarLoaded(Ok(_)))));
        assert!(events.next().is_none());

        // A move outside the browsed week must not rebuild that week.
        worker.subscriptions.register(
            SubscriptionId::Sidebar,
            Query::Sidebar {
                now: chrono_tz::UTC
                    .with_ymd_and_hms(2026, 11, 9, 0, 0, 0)
                    .unwrap(),
            },
        );
        let request = command(&worker, "2026-11-11T14:30:00");
        let change = worker.execute(Box::new(request), &test_runtime()).unwrap();
        assert_eq!(change.affected, vec![SubscriptionId::Sidebar]);
        assert!(matches!(
            worker
                .handle(
                    Message::RefreshSubscriptions(change.affected),
                    &test_runtime()
                )
                .as_slice(),
            [Event::SidebarLoaded(Ok(_))]
        ));

        let request = command(&worker, "2026-10-14T14:30:00");
        let change = worker.execute(Box::new(request), &test_runtime()).unwrap();
        assert!(
            change.affected.contains(&SubscriptionId::Calendar(1)),
            "entry into range is invalidated"
        );
    }

    #[test]
    fn responses_invalidate_visible_series_and_refresh_attendee_status() {
        use eventix_ical::objects::{CalOrganizer, CalPartStat, EventLike, UpdatableEventLike};
        use eventix_state::{EmailAccount, actions::items::RespondItem};
        use eventix_view::details::Response;

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker
            .state()
            .unwrap()
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .set_syncer(SyncerType::VDirSyncer {
                email: EmailAccount::new("Guest".into(), "guest@example.org".into()),
                url: "https://example.org".into(),
                read_only: false,
                username: None,
                password: None,
                time_span: Default::default(),
            });
        worker
            .state()
            .unwrap()
            .store_mut()
            .file_by_id_mut("move-me")
            .unwrap()
            .component_with_mut(|component| component.rid().is_none())
            .unwrap()
            .set_organizer(Some(CalOrganizer::new_named("Host", "host@example.org")));
        for recurrence in [None, Some("TU2026-10-13T09:00:00".parse().unwrap())] {
            subscribe(&mut worker, 1, "2026-10-12", View::Week);
            let item = ItemId {
                calendar: "test".into(),
                uid: "move-me".into(),
                recurrence,
            };
            let Some(Event::Changed(change)) = worker
                .handle(
                    Message::Execute(Box::new(RespondItem {
                        item: item.clone(),
                        response: CalPartStat::Accepted,
                    })),
                    &test_runtime(),
                )
                .pop()
            else {
                panic!("expected response notification")
            };
            assert_eq!(change.item, item);
            assert!(change.affected.contains(&SubscriptionId::Calendar(1)));
            assert!(change.affected.contains(&SubscriptionId::Sidebar));
            let events = worker.handle(
                Message::RefreshSubscriptions(change.affected),
                &test_runtime(),
            );
            let week = events
                .iter()
                .find_map(|event| match event {
                    Event::ContentLoaded(response) => match response.result.as_ref().unwrap() {
                        Snapshot::Week(week) => Some(week),
                        _ => None,
                    },
                    _ => None,
                })
                .unwrap();
            let event = &week.days[1].timed[0].event;
            assert_eq!(
                event.details.actions.series_response,
                Some(Response::Accepted)
            );
            assert_eq!(event.details.attendees[0].response, Response::Accepted);
        }
    }

    #[test]
    fn reminder_saves_return_the_selected_occurrence_and_refresh_series_membership() {
        use super::SavedItem;
        use eventix_ical::objects::CalCompType;
        use eventix_state::{CalendarAlarmType, actions::items::UpdatePersonalAlarms};

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker
            .state()
            .unwrap()
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .all_calendars_mut()
            .get_mut("test")
            .unwrap()
            .set_alarms(CalendarAlarmType::Personal { default: None });
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let selected = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: Some("TU2026-10-13T09:00:00".parse().unwrap()),
        };
        let base = ItemId {
            recurrence: None,
            ..selected.clone()
        };
        let Some(Event::DetailsLoaded(17, Ok(SavedItem::Event(base)))) = worker
            .handle(
                Message::LoadDetails(17, base, CalCompType::Event),
                &test_runtime(),
            )
            .pop()
        else {
            panic!("expected series reminder details")
        };
        assert!(base.id.recurrence.is_empty());
        assert!(base.recurrent);
        assert!(base.details.actions.personal_reminders);
        let command = UpdatePersonalAlarms {
            item: ItemId {
                recurrence: None,
                ..selected.clone()
            },
            alarms: Some(vec![]),
        };
        let Some(Event::RemindersSaved(Ok((change, SavedItem::Event(event))))) = worker
            .handle(
                Message::SaveReminders(selected.clone(), command),
                &test_runtime(),
            )
            .pop()
        else {
            panic!("expected saved reminder details")
        };
        assert!(change.item.recurrence.is_none());
        assert!(change.affected.contains(&SubscriptionId::Calendar(1)));
        assert!(change.affected.contains(&SubscriptionId::Sidebar));
        assert_eq!(
            event.id.recurrence,
            selected.recurrence.unwrap().to_string()
        );
        assert_eq!(event.details.reminders.as_deref(), Some("−"));
        assert_eq!(event.details.personal_reminders.series, Some(vec![]));
    }

    #[test]
    fn deletion_invalidates_old_calendar_and_sidebar_membership() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let item = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: None,
        };
        let change = worker
            .execute(
                Box::new(eventix_state::actions::items::DeleteItem { item: item.clone() }),
                &test_runtime(),
            )
            .unwrap();
        assert_eq!(change.item, item);
        assert!(change.affected.contains(&SubscriptionId::Calendar(1)));
        assert!(change.affected.contains(&SubscriptionId::Sidebar));
        let events = worker.handle(
            Message::RefreshSubscriptions(change.affected),
            &test_runtime(),
        );
        let week = events
            .iter()
            .find_map(|event| match event {
                Event::ContentLoaded(response) => match response.result.as_ref().unwrap() {
                    Snapshot::Week(week) => Some(week),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        assert!(week.days.iter().all(|day| day.timed.is_empty()));
        assert!(!tmp.path().join("event.ics").exists());
    }

    #[test]
    fn navigation_replaces_queries_and_obsolete_invalidations_do_not_restore_old_views() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let request = command(&worker, "2026-10-14T14:30:00");
        let change = worker.execute(Box::new(request), &test_runtime()).unwrap();
        subscribe(&mut worker, 2, "2026-11-01", View::Month);
        let old_calendar = change
            .affected
            .into_iter()
            .filter(|id| matches!(id, SubscriptionId::Calendar(_)))
            .collect();
        assert!(
            worker
                .handle(Message::RefreshSubscriptions(old_calendar), &test_runtime())
                .is_empty()
        );
        let request = command(&worker, "2026-11-10T14:30:00");
        let change = worker.execute(Box::new(request), &test_runtime()).unwrap();
        assert!(change.affected.contains(&SubscriptionId::Calendar(2)));
        assert!(!change.affected.contains(&SubscriptionId::Calendar(1)));
    }

    #[test]
    fn filtered_list_refreshes_for_changes_into_and_out_of_membership_and_deletion() {
        use eventix_state::actions::items::{DeleteItem, EditScope, UpdateItem};
        use eventix_view::list::Filter;

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker
            .state()
            .unwrap()
            .misc_mut()
            .toggle_calendar(&"test".into());
        let snapshot = worker
            .show_content(&ContentRequest {
                generation: 42,
                date: None,
                view: View::List,
                filter: Filter {
                    keywords: "needle".into(),
                    calendars: vec!["test".into()],
                },
            })
            .unwrap();
        assert!(matches!(snapshot, Snapshot::List(list) if list.items.is_empty()));
        let item = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: None,
        };
        for (summary, expected) in [("Needle", 1), ("Other", 0), ("Needle again", 1)] {
            let mut edit =
                UpdateItem::load(&worker.state().unwrap(), item.clone(), EditScope::Series)
                    .unwrap();
            edit.fields.summary = summary.into();
            let change = worker.execute(Box::new(edit), &test_runtime()).unwrap();
            assert!(change.affected.contains(&SubscriptionId::Calendar(42)));
            let events = worker.handle(
                Message::RefreshSubscriptions(change.affected),
                &test_runtime(),
            );
            assert!(matches!(events.as_slice(), [Event::ContentLoaded(response)]
                if response.generation == 42 && matches!(&response.result, Ok(Snapshot::List(list)) if list.items.len() == expected)));
        }
        let change = worker
            .execute(Box::new(DeleteItem { item }), &test_runtime())
            .unwrap();
        let events = worker.handle(
            Message::RefreshSubscriptions(change.affected),
            &test_runtime(),
        );
        assert!(matches!(events.as_slice(), [Event::ContentLoaded(response)]
            if matches!(&response.result, Ok(Snapshot::List(list)) if list.items.is_empty())));
        subscribe(&mut worker, 43, "2026-10-12", View::Week);
        assert!(
            worker
                .handle(
                    Message::RefreshSubscriptions(vec![SubscriptionId::Calendar(42)]),
                    &test_runtime()
                )
                .is_empty()
        );
    }

    #[test]
    fn list_rows_load_only_requested_identities_without_replacing_the_subscription() {
        use super::ListRowsRequest;

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        let Snapshot::List(index) = worker
            .show_content(&ContentRequest {
                generation: 42,
                date: None,
                view: View::List,
                filter: Default::default(),
            })
            .unwrap()
        else {
            panic!("expected a list index")
        };
        assert_eq!(index.items.len(), 1);
        let mut missing = index.items[0].clone();
        missing.uid = "missing".into();
        let events = worker.handle(
            Message::LoadListRows(ListRowsRequest {
                revision: 7,
                items: vec![index.items[0].clone(), missing.clone()],
            }),
            &test_runtime(),
        );
        let [Event::ListRowsLoaded(response)] = events.as_slice() else {
            panic!("expected row data")
        };
        assert_eq!(response.revision, 7);
        let rows = response.result.as_ref().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, index.items[0]);
        assert_eq!(rows[0].1.as_ref().unwrap().summary, "Move me");
        assert!(rows[0].1.as_ref().unwrap().recurrent);
        assert_eq!(rows[1].0, missing);
        assert!(rows[1].1.is_none());
        assert_eq!(worker.content.as_ref().unwrap().generation, 42);
        assert!(worker.subscriptions.contains(SubscriptionId::Calendar(42)));
    }

    #[test]
    fn calendar_toggles_persist_refresh_calendar_views_and_preserve_list_filters() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("task.ics"),
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\n\
             UID:unplanned\r\nSUMMARY:Task\r\nEND:VTODO\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let mut worker = worker(tmp.path());
        assert!(worker.calendars()[0].enabled);
        for view in [View::Week, View::Month] {
            subscribe(&mut worker, 7, "2026-10-12", view);
            for enabled in [false, true] {
                let events = worker.handle(Message::ToggleCalendar("test".into()), &test_runtime());
                let [
                    Event::CalendarsLoaded(calendars),
                    Event::ContentLoaded(response),
                    Event::SidebarLoaded(Ok(sidebar)),
                ] = events.as_slice()
                else {
                    panic!("toggle must refresh the header, calendar, and sidebar")
                };
                assert_eq!(calendars[0].enabled, enabled);
                assert_eq!(response.generation, 7);
                let count = match response.result.as_ref().unwrap() {
                    Snapshot::Week(week) => {
                        week.days.iter().map(|day| day.timed.len()).sum::<usize>()
                    }
                    Snapshot::Month(month) => month.days.iter().map(|day| day.events.len()).sum(),
                    Snapshot::List(_) => panic!("expected calendar view"),
                };
                assert_eq!(count > 0, enabled);
                assert_eq!(!sidebar.tasks.is_empty(), enabled);
                let saved = eventix_state::Misc::load_from_file(&worker.xdg).unwrap();
                assert_eq!(saved.calendar_disabled(&"test".into()), !enabled);
            }
        }

        let request = ContentRequest {
            generation: 8,
            date: None,
            view: View::List,
            filter: eventix_view::list::Filter {
                keywords: "Move".into(),
                calendars: vec!["test".into()],
            },
        };
        worker.show_content(&request).unwrap();
        let events = worker.handle(Message::ToggleCalendar("test".into()), &test_runtime());
        assert!(matches!(events.as_slice(),
            [Event::CalendarsLoaded(calendars), Event::SidebarLoaded(Ok(_))]
            if !calendars[0].enabled));
        let Snapshot::List(list) = worker.show_content(&request).unwrap() else {
            panic!("expected list")
        };
        assert_eq!(list.filter.keywords, "Move");
        assert_eq!(list.filter.calendars, vec!["test".to_string()]);
        assert_eq!(list.items.len(), 1);

        let events = worker.handle(Message::ToggleCalendar("missing".into()), &test_runtime());
        assert!(matches!(events.as_slice(),
            [Event::CalendarsLoaded(calendars), Event::CalendarToggleFailed(_)]
            if !calendars[0].enabled));
    }

    #[test]
    fn failed_calendar_visibility_write_restores_state_without_refreshing_views() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        fs::create_dir(tmp.path().join("misc.toml")).unwrap();
        let events = worker.handle(Message::ToggleCalendar("test".into()), &test_runtime());
        assert!(matches!(events.as_slice(),
            [Event::CalendarsLoaded(calendars), Event::CalendarToggleFailed(_)]
            if calendars[0].enabled));
        assert!(
            !worker
                .state()
                .unwrap()
                .misc()
                .calendar_disabled(&"test".into())
        );
    }

    #[test]
    fn list_can_initialize_first_and_reload_retains_selection_generation() {
        let tmp = tempfile::tempdir().unwrap();
        eventix_state::Settings::new(tmp.path().join("settings.toml"))
            .write_to_file()
            .unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let mut worker = Worker::new(xdg);
        let events = worker.handle(
            Message::ShowContent(ContentRequest {
                generation: 17,
                date: None,
                view: View::List,
                filter: eventix_view::list::Filter {
                    keywords: "needle".into(),
                    ..Default::default()
                },
            }),
            &test_runtime(),
        );
        assert!(
            matches!(events.as_slice(), [Event::StateLoaded, Event::PreferencesLoaded(_), Event::CalendarsLoaded(_), Event::ContentLoaded(response)]
            if response.generation == 17 && matches!(&response.result, Ok(Snapshot::List(list)) if list.items.is_empty()))
        );
        assert_eq!(worker.content.as_ref().unwrap().filter.keywords, "needle");
        let events = worker.handle(Message::Reload, &test_runtime());
        assert!(
            matches!(events.as_slice(), [Event::StateLoaded, Event::PreferencesLoaded(_), Event::CalendarsLoaded(_), Event::ContentLoaded(response)]
            if response.generation == 17 && matches!(&response.result, Ok(Snapshot::List(list)) if list.filter.keywords == "needle"))
        );
    }

    #[test]
    fn failed_commands_publish_no_changes_and_allow_subsequent_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let mut request = command(&worker, "2026-10-14T14:30:00");
        request.expected_start += Duration::minutes(15);
        assert!(matches!(
            worker
                .handle(Message::Execute(Box::new(request)), &test_runtime())
                .as_slice(),
            [Event::CommandFailed(_)]
        ));
        let request = command(&worker, "2026-10-14T14:30:00");
        assert!(matches!(
            worker
                .handle(Message::Execute(Box::new(request)), &test_runtime())
                .as_slice(),
            [Event::Changed(_)]
        ));
    }

    #[test]
    fn sidebar_and_content_requests_initialize_independently_and_keep_resolved_content() {
        let tmp = tempfile::tempdir().unwrap();
        eventix_state::Settings::new(tmp.path().join("settings.toml"))
            .write_to_file()
            .unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let mut worker = Worker::new(xdg);
        assert!(matches!(
            worker
                .handle(Message::RefreshSidebar, &test_runtime())
                .as_slice(),
            [
                Event::StateLoaded,
                Event::PreferencesLoaded(_),
                Event::CalendarsLoaded(_),
                Event::SidebarLoaded(Ok(_))
            ]
        ));
        assert!(worker.content.is_none());
        assert_eq!(worker.subscriptions.ids(), vec![SubscriptionId::Sidebar]);

        let events = worker.handle(
            Message::ShowContent(ContentRequest {
                generation: 1,
                date: None,
                view: View::Week,
                filter: Default::default(),
            }),
            &test_runtime(),
        );
        assert!(matches!(events.as_slice(),
            [Event::ContentLoaded(response)] if response.result.is_ok()));
        let content = worker.content.clone().unwrap();
        assert!(matches!(
            worker
                .handle(Message::RefreshSidebar, &test_runtime())
                .as_slice(),
            [Event::SidebarLoaded(Ok(_))]
        ));
        assert_eq!(worker.content.as_ref().unwrap().date, content.date);
        assert_eq!(
            worker.content.as_ref().unwrap().generation,
            content.generation
        );

        let events = worker.handle(
            Message::RefreshSubscriptions(vec![SubscriptionId::Calendar(1)]),
            &test_runtime(),
        );
        assert!(
            matches!(events.as_slice(), [Event::ContentLoaded(response)] if response.result.is_ok())
        );
        assert_eq!(worker.content.as_ref().unwrap().date, content.date);
    }

    #[test]
    fn failed_navigation_preserves_existing_subscriptions_and_sidebar_still_refreshes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let ids = worker.subscriptions.ids();
        let date = worker.content.as_ref().unwrap().date;
        let events = worker.handle(
            Message::ShowContent(ContentRequest {
                generation: 2,
                date: Some(NaiveDate::MAX),
                view: View::Week,
                filter: Default::default(),
            }),
            &test_runtime(),
        );
        assert!(matches!(events.as_slice(), [Event::ContentLoaded(response)]
            if response.generation == 2 && response.result.is_err()));
        assert_eq!(worker.content.as_ref().unwrap().date, date);
        assert_eq!(worker.subscriptions.ids(), ids);
        assert!(matches!(
            worker
                .handle(Message::RefreshSidebar, &test_runtime())
                .as_slice(),
            [Event::SidebarLoaded(Ok(_))]
        ));
        assert!(
            worker
                .handle(
                    Message::RefreshSubscriptions(vec![SubscriptionId::Calendar(2)]),
                    &test_runtime()
                )
                .is_empty()
        );
        assert!(
            matches!(worker.handle(Message::RefreshSubscriptions(ids), &test_runtime()).as_slice(),
            [Event::ContentLoaded(response), Event::SidebarLoaded(Ok(_))]
            if response.generation == 1 && response.result.is_ok())
        );
    }

    #[test]
    fn reload_replaces_state_from_disk_and_refreshes_active_subscriptions_without_navigation() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 7, "2026-10-12", View::Week);
        assert!(!worker.state().unwrap().store().directories().is_empty());
        // The on-disk profile has no calendars, unlike the current in-memory state.
        eventix_state::Settings::new(tmp.path().join("settings.toml"))
            .write_to_file()
            .unwrap();
        let events = worker.handle(Message::Reload, &test_runtime());
        let [
            Event::StateLoaded,
            Event::PreferencesLoaded(_),
            Event::CalendarsLoaded(_),
            Event::ContentLoaded(response),
            Event::SidebarLoaded(Ok(_)),
        ] = events.as_slice()
        else {
            panic!("reload must refresh content and sidebar independently")
        };
        assert_eq!(response.generation, 7);
        let Ok(Snapshot::Week(week)) = &response.result else {
            panic!("expected week")
        };
        assert_eq!(week.start.to_string(), "2026-10-12");
        assert!(week.days.iter().all(|day| day.timed.is_empty()));
        assert!(worker.state().unwrap().store().directories().is_empty());
        assert_eq!(worker.content.unwrap().date.to_string(), "2026-10-12");
        assert_eq!(
            worker.subscriptions.ids(),
            vec![SubscriptionId::Calendar(7), SubscriptionId::Sidebar]
        );
    }

    #[test]
    fn failed_reload_keeps_state_and_subscriptions_usable() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        fs::write(tmp.path().join("settings.toml"), "invalid TOML [").unwrap();
        let ids = worker.subscriptions.ids();
        assert!(matches!(
            worker.handle(Message::Reload, &test_runtime()).as_slice(),
            [Event::ReloadFailed(_)]
        ));
        assert_eq!(worker.subscriptions.ids(), ids);
        assert!(!worker.state().unwrap().store().directories().is_empty());
        assert!(
            matches!(worker.handle(Message::RefreshSubscriptions(ids), &test_runtime()).as_slice(),
            [Event::ContentLoaded(response), Event::SidebarLoaded(Ok(_))] if response.result.is_ok())
        );
        let command = command(&worker, "2026-10-14T14:30:00");
        assert!(worker.execute(Box::new(command), &test_runtime()).is_ok());
    }

    #[test]
    fn missing_profile_reports_expected_path_but_explicit_empty_profile_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let error = load_local_state(xdg.clone())
            .err()
            .expect("missing profile must fail");
        assert!(
            error
                .to_string()
                .contains(&tmp.path().join("settings.toml").display().to_string())
        );
        let settings = eventix_state::Settings::new(tmp.path().join("settings.toml"));
        settings.write_to_file().unwrap();
        let before = fs::read(tmp.path().join("settings.toml")).unwrap();
        let mut worker = Worker::new(xdg.clone());
        // A worker that failed before installing subscriptions must also recover on reload.
        let settings_path = tmp.path().join("settings.toml");
        fs::remove_file(&settings_path).unwrap();
        let events = worker.handle(
            Message::ShowContent(ContentRequest {
                generation: 1,
                date: Some("2026-10-12".parse().unwrap()),
                view: View::Week,
                filter: Default::default(),
            }),
            &test_runtime(),
        );
        assert!(matches!(events.as_slice(), [Event::ContentLoaded(response)]
            if response.generation == 1 && response.result.is_err()));
        assert!(worker.state.is_none());
        assert!(worker.subscriptions.ids().is_empty());
        fs::write(&settings_path, &before).unwrap();
        assert!(matches!(
            worker.handle(Message::Reload, &test_runtime()).as_slice(),
            [
                Event::StateLoaded,
                Event::PreferencesLoaded(_),
                Event::CalendarsLoaded(_)
            ]
        ));
        let events = worker.handle(
            Message::ShowContent(ContentRequest {
                generation: 1,
                date: Some("2026-10-12".parse().unwrap()),
                view: View::Week,
                filter: Default::default(),
            }),
            &test_runtime(),
        );
        assert!(matches!(events.as_slice(), [Event::ContentLoaded(response)]
            if response.generation == 1 && response.result.is_ok()));
        assert!(matches!(
            worker
                .handle(Message::RefreshSidebar, &test_runtime())
                .as_slice(),
            [Event::SidebarLoaded(Ok(_))]
        ));
        let state = load_local_state(xdg).unwrap();
        assert!(state.store().directories().is_empty());
        assert_eq!(fs::read(tmp.path().join("settings.toml")).unwrap(), before);
    }
}
