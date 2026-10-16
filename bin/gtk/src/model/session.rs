// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serialized queries and mutations; this module never accesses GTK objects.

use std::{sync::Arc, thread};

use chrono::{Duration, NaiveDate, Utc};
use eventix_state::actions::{
    Command,
    items::{CreateEvent, ItemId},
};
use eventix_view::{MonthSnapshot, WeekSnapshot};
use eventix_view::{editor::EditorOptions, sidebar::SidebarSnapshot};

use super::query::{Query, SubscriptionId, Subscriptions};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum View {
    #[default]
    Week,
    Month,
}

pub(crate) enum Snapshot {
    Week(WeekSnapshot),
    Month(MonthSnapshot),
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
}

pub(crate) struct ContentResponse {
    /// Identifies the content selection this result belongs to, including on failure.
    ///
    /// The page accepts the result only if this matches its current generation.
    pub generation: u64,
    pub result: Result<Snapshot, String>,
}

/// Notifications carry identities and invalidations; snapshots are refetched separately.
pub(crate) struct ChangeSet {
    pub items: Vec<ItemId>,
    pub affected: Vec<SubscriptionId>,
}

pub(crate) enum Event {
    EditorLoaded(u64, Result<EditorOptions, String>),
    EditorSaved(u64, Result<ChangeSet, String>),
    ContentLoaded(ContentResponse),
    SidebarLoaded(Result<SidebarSnapshot, String>),
    SourceLoaded(String),
    ReloadFailed(String),
    Changed(ChangeSet),
    CommandFailed(String),
}

#[derive(Debug)]
enum Message {
    PrepareEditor(u64),
    CreateEvent(u64, Box<CreateEvent>),
    ShowContent(ContentRequest),
    RefreshSidebar,
    RefreshSubscriptions(Vec<SubscriptionId>),
    Reload,
    Execute(Command),
}

/// Owns the request channel, closing the worker when the page stops or is dropped.
pub(crate) struct CalendarSession {
    requests: async_channel::Sender<Message>,
}

impl CalendarSession {
    pub fn prepare_editor(&self, id: u64) {
        let _ = self.requests.try_send(Message::PrepareEditor(id));
    }

    pub fn create_event(&self, id: u64, event: Box<CreateEvent>) {
        let _ = self.requests.try_send(Message::CreateEvent(id, event));
    }

    pub fn start(prefix: String) -> (Self, async_channel::Receiver<Event>) {
        let (requests, rx) = async_channel::unbounded::<Message>();
        let (results, responses) = async_channel::unbounded();
        thread::spawn(move || {
            let xdg = Arc::new(xdg::BaseDirectories::with_prefix(prefix));
            let mut worker = Worker::new(xdg);
            // Queries and commands are ordered. Never discard a queued mutation.
            while let Ok(message) = rx.recv_blocking() {
                for event in worker.handle(message) {
                    if results.send_blocking(event).is_err() {
                        return;
                    }
                }
            }
        });
        (Self { requests }, responses)
    }

    pub fn show_content(&self, request: ContentRequest) {
        let _ = self.requests.try_send(Message::ShowContent(request));
    }

    pub fn refresh_sidebar(&self) {
        let _ = self.requests.try_send(Message::RefreshSidebar);
    }

    pub fn reload(&self) {
        let _ = self.requests.try_send(Message::Reload);
    }

    pub fn execute(&self, command: Command) {
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

#[derive(Clone, Copy)]
struct ContentSubscription {
    /// Retains the selection's generation when rebuilding its snapshot or query.
    generation: u64,
    date: NaiveDate,
    view: View,
}

impl ContentSubscription {
    fn query(&self, state: &eventix_state::State) -> anyhow::Result<Query> {
        let (start, end) = match self.view {
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
        }
    }
}

struct Worker {
    xdg: Arc<xdg::BaseDirectories>,
    state: Option<eventix_state::State>,
    content: Option<ContentSubscription>,
    subscriptions: Subscriptions,
}

impl Worker {
    fn new(xdg: Arc<xdg::BaseDirectories>) -> Self {
        Self {
            xdg,
            state: None,
            content: None,
            subscriptions: Subscriptions::default(),
        }
    }

    fn handle(&mut self, message: Message) -> Vec<Event> {
        tracing::debug!("handle(message={:?})", message);
        let mut events = Vec::new();

        // load state first if not already done and the message requires it
        if self.state.is_none()
            && matches!(
                message,
                Message::ShowContent(_) | Message::RefreshSidebar | Message::PrepareEditor(_)
            )
        {
            match load_local_state(self.xdg.clone()) {
                Ok(state) => {
                    events.push(Event::SourceLoaded(self.source_description(&state)));
                    self.state = Some(state);
                }
                Err(error) => {
                    let error = format!("{error:#}");
                    return vec![match message {
                        Message::ShowContent(request) => Event::ContentLoaded(ContentResponse {
                            generation: request.generation,
                            result: Err(error),
                        }),
                        Message::PrepareEditor(id) => Event::EditorLoaded(id, Err(error)),
                        _ => Event::SidebarLoaded(Err(error)),
                    }];
                }
            }
        }

        match message {
            Message::PrepareEditor(id) => events.push(Event::EditorLoaded(
                id,
                self.state()
                    .and_then(EditorOptions::load)
                    .map_err(|error| format!("{error:#}")),
            )),
            Message::CreateEvent(id, event) => events.push(Event::EditorSaved(
                id,
                self.execute(Command::CreateEvent(event))
                    .map_err(|error| format!("{error:#}")),
            )),
            Message::ShowContent(request) => {
                let result = self
                    .show_content(&request)
                    .map_err(|error| format!("{error:#}"));
                events.push(Event::ContentLoaded(ContentResponse {
                    generation: request.generation,
                    result,
                }));
            }
            Message::RefreshSidebar => events.push(self.refresh_sidebar()),
            Message::RefreshSubscriptions(ids) => events.extend(self.refresh_subscriptions(&ids)),
            Message::Reload => match load_local_state(self.xdg.clone()) {
                Ok(state) => {
                    events.push(Event::SourceLoaded(self.source_description(&state)));
                    self.state = Some(state);
                    let ids = self.subscriptions.ids();
                    events.extend(self.refresh_subscriptions(&ids));
                }
                Err(error) => events.push(Event::ReloadFailed(format!("{error:#}"))),
            },
            Message::Execute(command) => events.push(match self.execute(command) {
                Ok(change) => Event::Changed(change),
                Err(error) => Event::CommandFailed(format!("{error:#}")),
            }),
        }
        events
    }

    fn state(&self) -> anyhow::Result<&eventix_state::State> {
        self.state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No loaded state"))
    }

    fn show_content(&mut self, request: &ContentRequest) -> anyhow::Result<Snapshot> {
        let state = self.state()?;
        let content = ContentSubscription {
            generation: request.generation,
            date: request
                .date
                .unwrap_or_else(|| Utc::now().with_timezone(state.timezone()).date_naive()),
            view: request.view,
        };
        self.load_content(content)
    }

    /// Installs a subscription only after its query and initial snapshot are available.
    fn load_content(&mut self, content: ContentSubscription) -> anyhow::Result<Snapshot> {
        let state = self.state()?;
        let query = content.query(state)?;
        let snapshot = content.snapshot(state)?;
        self.subscriptions
            .register(SubscriptionId::Calendar(content.generation), query);
        self.content = Some(content);
        Ok(snapshot)
    }

    fn refresh_sidebar(&mut self) -> Event {
        let result = (|| -> anyhow::Result<SidebarSnapshot> {
            let state = self.state()?;
            let now = Utc::now().with_timezone(state.timezone());
            let snapshot = SidebarSnapshot::load(state, now);
            self.subscriptions
                .register(SubscriptionId::Sidebar, Query::Sidebar { now });
            Ok(snapshot)
        })()
        .map_err(|error| format!("{error:#}"));
        Event::SidebarLoaded(result)
    }

    fn refresh_subscriptions(&mut self, ids: &[SubscriptionId]) -> Vec<Event> {
        let mut events = Vec::new();

        if let Some(content) = self.content
            && ids.contains(&SubscriptionId::Calendar(content.generation))
        {
            let result = self
                .load_content(content)
                .map_err(|error| format!("{error:#}"));
            events.push(Event::ContentLoaded(ContentResponse {
                generation: content.generation,
                result,
            }));
        }

        if ids.contains(&SubscriptionId::Sidebar)
            && self.subscriptions.contains(SubscriptionId::Sidebar)
        {
            events.push(self.refresh_sidebar());
        }

        events
    }

    fn source_description(&self, state: &eventix_state::State) -> String {
        let config = self
            .xdg
            .find_config_file("settings.toml")
            .or_else(|| self.xdg.get_config_file("settings.toml"))
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let data = self
            .xdg
            .get_data_home()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        format!(
            "{}\n{}",
            formatx::formatx!(
                state.locale().translate("Calendar configuration: {}"),
                config
            )
            .unwrap(),
            formatx::formatx!(state.locale().translate("Calendar data: {}"), data).unwrap()
        )
    }

    fn execute(&mut self, command: Command) -> anyhow::Result<ChangeSet> {
        let state = self
            .state
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No loaded state"))?;

        let mut affected = Vec::new();
        for item in command.affected_items() {
            for id in self.subscriptions.matching(state, item) {
                if !affected.contains(&id) {
                    affected.push(id);
                }
            }
        }

        let items = command.execute(state)?;
        for item in &items {
            for id in self.subscriptions.matching(state, item) {
                if !affected.contains(&id) {
                    affected.push(id);
                }
            }
        }

        Ok(ChangeSet { items, affected })
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
        Command, ContentRequest, Event, ItemId, Message, Query, Snapshot, SubscriptionId, View,
        Worker, load_local_state,
    };

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
        worker.state = Some(state);
        worker
    }

    fn command(worker: &Worker, start: &str) -> MoveEvent {
        let item = ItemId {
            calendar: "test".into(),
            uid: "move-me".into(),
            recurrence: Some("TU2026-10-13T09:00:00".parse().unwrap()),
        };
        let state = worker.state.as_ref().unwrap();
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
    fn editor_requests_preserve_identity_and_creation_invalidates_matching_queries() {
        use eventix_ical::objects::{CalCompType, CalDate, CalDateType};
        use eventix_state::actions::items::CreateEvent;

        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        worker
            .state
            .as_mut()
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
        let Some(Event::EditorLoaded(42, Ok(options))) =
            worker.handle(Message::PrepareEditor(42)).pop()
        else {
            panic!("editor options should echo the request identity");
        };
        assert_eq!(options.calendars.len(), 1);
        assert_eq!(options.calendars[0].id, "test");
        let make_request = || {
            Box::new(CreateEvent {
                calendar: "test".into(),
                summary: "Created".into(),
                location: String::new(),
                description: String::new(),
                start: CalDate::new_date("2026-10-14".parse().unwrap(), CalDateType::Inclusive),
                end: CalDate::new_date("2026-10-15".parse().unwrap(), CalDateType::Exclusive),
                recurrence: None,
                alarms: Vec::new(),
                personal_alarms: None,
                attendees: Vec::new(),
            })
        };
        let mut invalid = make_request();
        invalid.summary.clear();
        assert!(matches!(
            worker.handle(Message::CreateEvent(43, invalid)).as_slice(),
            [Event::EditorSaved(43, Err(_))]
        ));
        let Some(Event::EditorSaved(44, Ok(change))) = worker
            .handle(Message::CreateEvent(44, make_request()))
            .pop()
        else {
            panic!("successful save must publish the request identity and committed changes");
        };
        assert_eq!(change.items.len(), 1);
        assert!(change.affected.contains(&SubscriptionId::Calendar(1)));
        assert!(change.affected.contains(&SubscriptionId::Sidebar));
        assert!(
            matches!(worker.handle(Message::RefreshSubscriptions(change.affected)).as_slice(),
            [Event::ContentLoaded(response), Event::SidebarLoaded(Ok(_))] if response.result.is_ok())
        );
    }

    fn subscribe(worker: &mut Worker, generation: u64, date: &str, view: View) {
        let response = worker.show_content(&ContentRequest {
            generation,
            date: Some(date.parse().unwrap()),
            view,
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
    fn moves_invalidate_old_and_new_query_membership_and_refetch_only_affected_views() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let request = command(&worker, "2026-11-10T14:30:00");
        let Some(Event::Changed(change)) = worker
            .handle(Message::Execute(Command::MoveEvent(request)))
            .pop()
        else {
            panic!("expected committed identity notification")
        };
        assert_eq!(change.items.len(), 1);
        assert!(
            change.affected.contains(&SubscriptionId::Calendar(1)),
            "old range is invalidated"
        );
        assert!(
            change.affected.contains(&SubscriptionId::Sidebar),
            "old sidebar membership is invalidated"
        );
        let mut events = worker
            .handle(Message::RefreshSubscriptions(change.affected))
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
        let change = worker.execute(Command::MoveEvent(request)).unwrap();
        assert_eq!(change.affected, vec![SubscriptionId::Sidebar]);
        assert!(matches!(
            worker
                .handle(Message::RefreshSubscriptions(change.affected))
                .as_slice(),
            [Event::SidebarLoaded(Ok(_))]
        ));

        let request = command(&worker, "2026-10-14T14:30:00");
        let change = worker.execute(Command::MoveEvent(request)).unwrap();
        assert!(
            change.affected.contains(&SubscriptionId::Calendar(1)),
            "entry into range is invalidated"
        );
    }

    #[test]
    fn navigation_replaces_queries_and_obsolete_invalidations_do_not_restore_old_views() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let request = command(&worker, "2026-10-14T14:30:00");
        let change = worker.execute(Command::MoveEvent(request)).unwrap();
        subscribe(&mut worker, 2, "2026-11-01", View::Month);
        let old_calendar = change
            .affected
            .into_iter()
            .filter(|id| matches!(id, SubscriptionId::Calendar(_)))
            .collect();
        assert!(
            worker
                .handle(Message::RefreshSubscriptions(old_calendar))
                .is_empty()
        );
        let request = command(&worker, "2026-11-10T14:30:00");
        let change = worker.execute(Command::MoveEvent(request)).unwrap();
        assert!(change.affected.contains(&SubscriptionId::Calendar(2)));
        assert!(!change.affected.contains(&SubscriptionId::Calendar(1)));
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
                .handle(Message::Execute(Command::MoveEvent(request)))
                .as_slice(),
            [Event::CommandFailed(_)]
        ));
        let request = command(&worker, "2026-10-14T14:30:00");
        assert!(matches!(
            worker
                .handle(Message::Execute(Command::MoveEvent(request)))
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
            worker.handle(Message::RefreshSidebar).as_slice(),
            [Event::SourceLoaded(_), Event::SidebarLoaded(Ok(_))]
        ));
        assert!(worker.content.is_none());
        assert_eq!(worker.subscriptions.ids(), vec![SubscriptionId::Sidebar]);

        let events = worker.handle(Message::ShowContent(ContentRequest {
            generation: 1,
            date: None,
            view: View::Week,
        }));
        assert!(matches!(events.as_slice(),
            [Event::ContentLoaded(response)] if response.result.is_ok()));
        let content = worker.content.unwrap();
        assert!(matches!(
            worker.handle(Message::RefreshSidebar).as_slice(),
            [Event::SidebarLoaded(Ok(_))]
        ));
        assert_eq!(worker.content.unwrap().date, content.date);
        assert_eq!(worker.content.unwrap().generation, content.generation);

        let events = worker.handle(Message::RefreshSubscriptions(vec![
            SubscriptionId::Calendar(1),
        ]));
        assert!(
            matches!(events.as_slice(), [Event::ContentLoaded(response)] if response.result.is_ok())
        );
        assert_eq!(worker.content.unwrap().date, content.date);
    }

    #[test]
    fn failed_navigation_preserves_existing_subscriptions_and_sidebar_still_refreshes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let ids = worker.subscriptions.ids();
        let date = worker.content.unwrap().date;
        let events = worker.handle(Message::ShowContent(ContentRequest {
            generation: 2,
            date: Some(NaiveDate::MAX),
            view: View::Week,
        }));
        assert!(matches!(events.as_slice(), [Event::ContentLoaded(response)]
            if response.generation == 2 && response.result.is_err()));
        assert_eq!(worker.content.unwrap().date, date);
        assert_eq!(worker.subscriptions.ids(), ids);
        assert!(matches!(
            worker.handle(Message::RefreshSidebar).as_slice(),
            [Event::SidebarLoaded(Ok(_))]
        ));
        assert!(
            worker
                .handle(Message::RefreshSubscriptions(vec![
                    SubscriptionId::Calendar(2)
                ]))
                .is_empty()
        );
        assert!(
            matches!(worker.handle(Message::RefreshSubscriptions(ids)).as_slice(),
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
        let events = worker.handle(Message::Reload);
        let [
            Event::SourceLoaded(_),
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
            worker.handle(Message::Reload).as_slice(),
            [Event::ReloadFailed(_)]
        ));
        assert_eq!(worker.subscriptions.ids(), ids);
        assert!(!worker.state().unwrap().store().directories().is_empty());
        assert!(
            matches!(worker.handle(Message::RefreshSubscriptions(ids)).as_slice(),
            [Event::ContentLoaded(response), Event::SidebarLoaded(Ok(_))] if response.result.is_ok())
        );
        let command = command(&worker, "2026-10-14T14:30:00");
        assert!(worker.execute(Command::MoveEvent(command)).is_ok());
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
        let events = worker.handle(Message::ShowContent(ContentRequest {
            generation: 1,
            date: Some("2026-10-12".parse().unwrap()),
            view: View::Week,
        }));
        assert!(matches!(events.as_slice(), [Event::ContentLoaded(response)]
            if response.generation == 1 && response.result.is_err()));
        assert!(worker.state.is_none());
        assert!(worker.subscriptions.ids().is_empty());
        fs::write(&settings_path, &before).unwrap();
        assert!(matches!(
            worker.handle(Message::Reload).as_slice(),
            [Event::SourceLoaded(_)]
        ));
        let events = worker.handle(Message::ShowContent(ContentRequest {
            generation: 1,
            date: Some("2026-10-12".parse().unwrap()),
            view: View::Week,
        }));
        assert!(matches!(events.as_slice(), [Event::ContentLoaded(response)]
            if response.generation == 1 && response.result.is_ok()));
        assert!(matches!(
            worker.handle(Message::RefreshSidebar).as_slice(),
            [Event::SidebarLoaded(Ok(_))]
        ));
        let state = load_local_state(xdg).unwrap();
        assert!(state.store().directories().is_empty());
        assert_eq!(fs::read(tmp.path().join("settings.toml")).unwrap(), before);
    }
}
