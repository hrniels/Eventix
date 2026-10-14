// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serialized queries and mutations; this module never accesses GTK objects.

use std::{sync::Arc, thread};

use chrono::{Duration, NaiveDate, Utc};
use eventix_state::actions::{Command, items::ItemId};
use eventix_view::sidebar::SidebarSnapshot;
use eventix_view::{MonthSnapshot, WeekSnapshot};

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
    Sidebar,
}

#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub generation: u64,
    pub date: Option<NaiveDate>,
    pub reload: bool,
    pub view: View,
    pub sidebar_only: bool,
}

pub(crate) struct Response {
    pub generation: u64,
    pub result: Result<(Snapshot, String), String>,
    pub sidebar: Option<SidebarSnapshot>,
}

/// Notifications carry identities and invalidations; snapshots are refetched separately.
pub(crate) struct ChangeSet {
    pub revision: u64,
    pub items: Vec<ItemId>,
    pub affected: Vec<SubscriptionId>,
}

pub(crate) enum Event {
    Loaded(Response),
    Changed(ChangeSet),
    CommandFailed(String),
}

#[derive(Debug)]
enum Message {
    Subscribe(Request),
    Execute(Command),
    Refresh(Vec<SubscriptionId>),
}

/// Owns the request channel, closing the worker when the page stops or is dropped.
pub(crate) struct CalendarSession {
    requests: async_channel::Sender<Message>,
}

impl CalendarSession {
    pub fn start(prefix: String) -> (Self, async_channel::Receiver<Event>) {
        let (requests, rx) = async_channel::unbounded::<Message>();
        let (results, responses) = async_channel::unbounded();
        thread::spawn(move || {
            let xdg = Arc::new(xdg::BaseDirectories::with_prefix(prefix));
            let mut worker = Worker::new(xdg);
            // Queries and commands are ordered. Never discard a queued mutation.
            while let Ok(message) = rx.recv_blocking() {
                if let Some(event) = worker.handle(message)
                    && results.send_blocking(event).is_err()
                {
                    break;
                }
            }
        });
        (Self { requests }, responses)
    }

    pub fn request(&self, request: Request) {
        let _ = self.requests.try_send(Message::Subscribe(request));
    }

    pub fn execute(&self, command: Command) {
        let _ = self.requests.try_send(Message::Execute(command));
    }

    pub fn refresh(&self, subscriptions: Vec<SubscriptionId>) {
        let _ = self.requests.try_send(Message::Refresh(subscriptions));
    }
}

impl Drop for CalendarSession {
    fn drop(&mut self) {
        self.requests.close();
    }
}

struct Worker {
    xdg: Arc<xdg::BaseDirectories>,
    state: Option<eventix_state::State>,
    request: Option<Request>,
    subscriptions: Subscriptions,
    revision: u64,
}

impl Worker {
    fn new(xdg: Arc<xdg::BaseDirectories>) -> Self {
        Self {
            xdg,
            state: None,
            request: None,
            subscriptions: Subscriptions::default(),
            revision: 0,
        }
    }

    fn handle(&mut self, message: Message) -> Option<Event> {
        tracing::debug!("handle(message={:?})", message);
        match message {
            Message::Subscribe(request) => {
                let calendar = !request.sidebar_only;
                Some(Event::Loaded(self.load(request, calendar, true)))
            }
            Message::Refresh(ids) => {
                let request = self.request.clone()?;
                let calendar = ids.contains(&SubscriptionId::Calendar(request.generation));
                let sidebar = ids.contains(&SubscriptionId::Sidebar);
                (calendar || sidebar).then(|| Event::Loaded(self.load(request, calendar, sidebar)))
            }
            Message::Execute(command) => Some(match self.execute(command) {
                Ok(change) => Event::Changed(change),
                Err(error) => Event::CommandFailed(format!("{error:#}")),
            }),
        }
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
        self.revision += 1;
        Ok(ChangeSet {
            revision: self.revision,
            items,
            affected,
        })
    }

    fn load(&mut self, mut request: Request, calendar: bool, refresh_sidebar: bool) -> Response {
        let mut sidebar = None;
        let result = (|| -> anyhow::Result<(Snapshot, String)> {
            if self.state.is_none() || request.reload {
                self.state = Some(load_local_state(self.xdg.clone())?);
            }
            let state = self.state.as_ref().expect("state was successfully loaded");
            let now = Utc::now().with_timezone(state.timezone());
            let date = request.date.unwrap_or_else(|| now.date_naive());
            request.date = Some(date);
            request.reload = false;
            if refresh_sidebar {
                sidebar = Some(SidebarSnapshot::load(state, now));
                self.subscriptions
                    .register(SubscriptionId::Sidebar, Query::Sidebar { now });
            }
            let snapshot = if calendar {
                let (start, end) = match request.view {
                    View::Week => {
                        let start = eventix_view::week_start(date)
                            .ok_or_else(|| anyhow::anyhow!("Week start out of range"))?;
                        (
                            start,
                            start
                                .checked_add_signed(Duration::days(7))
                                .ok_or_else(|| anyhow::anyhow!("Week end out of range"))?,
                        )
                    }
                    View::Month => eventix_view::month_range(date)
                        .ok_or_else(|| anyhow::anyhow!("Month out of range"))?,
                };
                let snapshot = match request.view {
                    View::Week => Snapshot::Week(WeekSnapshot::load(state, date)?),
                    View::Month => Snapshot::Month(MonthSnapshot::load(state, date)?),
                };
                let at_midnight = |date: NaiveDate| {
                    eventix_ical::util::resolve_local_time(
                        state.timezone(),
                        date.and_hms_opt(0, 0, 0).unwrap(),
                    )
                };
                self.subscriptions.register(
                    SubscriptionId::Calendar(request.generation),
                    Query::Events {
                        start: at_midnight(start),
                        end: at_midnight(end),
                    },
                );
                snapshot
            } else {
                Snapshot::Sidebar
            };
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
            let source = format!(
                "{}\n{}",
                formatx::formatx!(
                    state.locale().translate("Calendar configuration: {}"),
                    config
                )
                .unwrap(),
                formatx::formatx!(state.locale().translate("Calendar data: {}"), data).unwrap()
            );
            self.request = Some(request.clone());
            Ok((snapshot, source))
        })()
        .map_err(|error| format!("{error:#}"));
        Response {
            generation: request.generation,
            result,
            sidebar,
        }
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

    use chrono::{Duration, TimeZone};
    use eventix_ical::col::CalDir;
    use eventix_state::actions::items::MoveEvent;
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};

    use super::{
        Command, Event, ItemId, Message, Query, Request, Snapshot, SubscriptionId, View, Worker,
        load_local_state,
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

    fn subscribe(worker: &mut Worker, generation: u64, date: &str, view: View) {
        let response = worker.load(
            Request {
                generation,
                date: Some(date.parse().unwrap()),
                reload: false,
                view,
                sidebar_only: false,
            },
            true,
            true,
        );
        assert!(response.result.is_ok());
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
        let Some(Event::Changed(change)) =
            worker.handle(Message::Execute(Command::MoveEvent(request)))
        else {
            panic!("expected committed identity notification")
        };
        assert_eq!(change.revision, 1);
        assert_eq!(change.items.len(), 1);
        assert!(
            change.affected.contains(&SubscriptionId::Calendar(1)),
            "old range is invalidated"
        );
        assert!(
            change.affected.contains(&SubscriptionId::Sidebar),
            "old sidebar membership is invalidated"
        );
        let Some(Event::Loaded(response)) = worker.handle(Message::Refresh(change.affected)) else {
            panic!("expected query refresh")
        };
        let (Snapshot::Week(week), _) = response.result.unwrap() else {
            panic!("expected week")
        };
        assert_eq!(
            week.days[1].timed.len(),
            0,
            "moved exception left the old day"
        );

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
        let Some(Event::Loaded(response)) = worker.handle(Message::Refresh(change.affected)) else {
            panic!("expected sidebar")
        };
        assert!(matches!(response.result.unwrap().0, Snapshot::Sidebar));
        assert!(response.sidebar.is_some());

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
        assert!(worker.handle(Message::Refresh(old_calendar)).is_none());
        let request = command(&worker, "2026-11-10T14:30:00");
        let change = worker.execute(Command::MoveEvent(request)).unwrap();
        assert!(change.affected.contains(&SubscriptionId::Calendar(2)));
        assert!(!change.affected.contains(&SubscriptionId::Calendar(1)));
    }

    #[test]
    fn failed_commands_publish_no_changes_and_do_not_advance_revision() {
        let tmp = tempfile::tempdir().unwrap();
        let mut worker = worker(tmp.path());
        subscribe(&mut worker, 1, "2026-10-12", View::Week);
        let mut request = command(&worker, "2026-10-14T14:30:00");
        request.expected_start += Duration::minutes(15);
        assert!(matches!(
            worker.handle(Message::Execute(Command::MoveEvent(request))),
            Some(Event::CommandFailed(_))
        ));
        assert_eq!(worker.revision, 0);
        let request = command(&worker, "2026-10-14T14:30:00");
        assert_eq!(
            worker
                .execute(Command::MoveEvent(request))
                .unwrap()
                .revision,
            1
        );
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
        let state = load_local_state(xdg).unwrap();
        assert!(state.store().directories().is_empty());
        assert_eq!(fs::read(tmp.path().join("settings.toml")).unwrap(), before);
    }
}
