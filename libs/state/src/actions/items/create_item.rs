// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::bail;
use eventix_ical::objects::{
    CalAlarm, CalAttendee, CalCompType, CalDate, CalDateTime, CalRRule, CalTodoStatus, DateContext,
    UpdatableEventLike,
};

use super::{ItemId, create_component};
use crate::{CalendarAlarmType, State};

/// Creates an event or task from frontend-independent, already parsed field values.
#[derive(Clone, Debug)]
pub struct CreateItem {
    pub kind: CalCompType,
    pub task_status: Option<TaskStatus>,
    pub calendar: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    /// Required for events and recurring tasks; optional for other tasks.
    pub start: Option<CalDate>,
    /// The exclusive event end or inclusive task due date, which is optional for tasks.
    pub end: Option<CalDate>,
    pub recurrence: Option<CalRRule>,
    pub alarms: Vec<CalAlarm>,
    pub personal_alarms: Option<Vec<CalAlarm>>,
    pub attendees: Vec<CalAttendee>,
}

/// Editable task status, progress, and completion date.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskStatus {
    pub status: CalTodoStatus,
    pub percent: Option<u8>,
    pub completed: Option<CalDate>,
}

impl TaskStatus {
    pub(super) fn apply(&self, task: &mut eventix_ical::objects::CalTodo) {
        task.set_status(Some(self.status));
        task.set_percent(match self.status {
            CalTodoStatus::Completed => Some(100),
            CalTodoStatus::InProcess => self.percent,
            _ => None,
        });
        task.set_completed(if self.status == CalTodoStatus::Completed {
            self.completed.as_ref().map(CalDate::to_utc)
        } else {
            None
        });
    }
}

impl CreateItem {
    /// Validates and persists an event or task, returning its new identity.
    pub fn execute(self, state: &mut State) -> anyhow::Result<ItemId> {
        self.validate(state)?;
        self.create(state)
    }

    pub(super) fn validate(&self, state: &State) -> anyhow::Result<()> {
        let locale = state.locale();

        if self.summary.trim().is_empty() {
            bail!(locale.translate("error.summary_empty").to_string());
        }

        if self.kind == CalCompType::Event && (self.start.is_none() || self.end.is_none()) {
            bail!(locale.translate("error.valid_date_time").to_string());
        }
        if let (Some(start), Some(end)) = (&self.start, &self.end)
            && matches!(start, CalDate::Date(..)) != matches!(end, CalDate::Date(..))
        {
            bail!(locale.translate("error.time_for_both_or_none").to_string());
        }

        for date in [&self.start, &self.end].into_iter().flatten() {
            if let CalDate::DateTime(CalDateTime::Timezone(_, name)) = date {
                name.parse::<chrono_tz::Tz>().map_err(|_| {
                    anyhow::anyhow!(locale.translate("error.valid_date_time").to_string())
                })?;
            }
        }

        let context = DateContext::system();
        if let (Some(start), Some(end)) = (&self.start, &self.end)
            && context.date_to_utc(start, state.timezone())
                > context.date_to_utc(end, state.timezone())
        {
            bail!(locale.translate("error.end_before_start").to_string());
        }

        if let Some(rule) = &self.recurrence {
            let Some(start) = &self.start else {
                bail!(locale.translate("Invalid recurrence").to_string());
            };
            if rule.interval() == Some(0) || rule.count() == Some(0) {
                bail!(locale.translate("Invalid recurrence").to_string());
            }

            if let Some(until) = rule.until()
                && context.date_to_utc(until, state.timezone())
                    < context.date_to_utc(start, state.timezone())
            {
                bail!(locale.translate("error.end_before_start").to_string());
            }
        }

        if self
            .task_status
            .as_ref()
            .is_some_and(|status| status.percent.is_some_and(|percent| percent > 100))
        {
            bail!(locale.translate("Could not apply changes").to_string());
        }
        Ok(())
    }

    fn create(self, state: &mut State) -> anyhow::Result<ItemId> {
        let locale = state.locale();
        let personal_alarms = self.personal_alarms;
        let identity = create_component(
            state,
            &locale,
            &self.calendar,
            self.kind,
            |_, _, component, _, organizer, _, _| {
                component.set_summary(Some(self.summary));
                component.set_location((!self.location.is_empty()).then_some(self.location));
                component
                    .set_description((!self.description.is_empty()).then_some(self.description));
                component.set_start(self.start);
                if let Some(event) = component.as_event_mut() {
                    event.set_end(self.end);
                } else if let Some(task) = component.as_todo_mut() {
                    task.set_due(self.end);
                    if let Some(status) = &self.task_status {
                        status.apply(task);
                    }
                    task.set_priority(Some(eventix_ical::objects::PRIORITY_MEDIUM));
                }
                component.set_rrule(self.recurrence);
                component.set_alarms((!self.alarms.is_empty()).then_some(self.alarms));
                if !self.attendees.is_empty() {
                    component.set_organizer(organizer);
                    component.set_attendees(Some(self.attendees));
                }
                component.touch();
                Ok(())
            },
        )?;

        if let Some(alarms) = personal_alarms
            && state
                .settings()
                .calendar(&identity.calendar)
                .is_some_and(|(_, settings)| {
                    matches!(settings.alarms(), CalendarAlarmType::Personal { .. })
                })
        {
            let personal = state
                .personal_alarms_mut()
                .get_or_create(&identity.calendar);
            personal.set(&identity.uid, None, alarms);
            if let Err(error) = personal.save() {
                tracing::warn!(%error, "Unable to save personal alarms");
            }
        }

        Ok(identity)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use eventix_ical::{
        col::CalDir,
        objects::{CalCompType, CalDateType, CalRole, EventLike},
    };

    use super::{CalAlarm, CalAttendee, CalDate, CalRRule, CreateItem, State};
    use crate::{CalendarSettings, CollectionSettings, SyncerType};

    fn state(path: &Path, read_only: bool) -> State {
        let xdg = Arc::new(crate::with_test_xdg(path, path));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        state.store_mut().add(CalDir::new_empty(
            Arc::new("test".into()),
            path.into(),
            "Test".into(),
            read_only,
        ));
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: path.display().to_string(),
        });
        let mut settings = CalendarSettings::default();
        settings.set_enabled(true);
        settings.set_types(vec![CalCompType::Event, CalCompType::Todo]);
        collection
            .all_calendars_mut()
            .insert("test".into(), settings);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        state
    }

    fn command() -> CreateItem {
        CreateItem {
            kind: CalCompType::Event,
            task_status: None,
            calendar: "test".into(),
            summary: "New event".into(),
            location: "Office".into(),
            description: "Some details\nSecond line".into(),
            start: Some("TTEurope/Berlin;2026-10-15T10:00:00".parse().unwrap()),
            end: Some("TTEurope/Berlin;2026-10-15T11:00:00".parse().unwrap()),
            recurrence: None,
            alarms: Vec::new(),
            personal_alarms: None,
            attendees: Vec::new(),
        }
    }

    #[test]
    fn tasks_allow_independent_dates_and_persist_inclusive_due_dates_and_status() {
        for (has_start, has_due) in [(false, false), (true, false), (false, true), (true, true)] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = state(tmp.path(), false);
            let mut request = command();
            request.kind = CalCompType::Todo;
            request.start = has_start
                .then(|| CalDate::new_date("2026-10-15".parse().unwrap(), CalDateType::Inclusive));
            request.end = has_due
                .then(|| CalDate::new_date("2026-10-15".parse().unwrap(), CalDateType::Inclusive));
            request.task_status = Some(super::TaskStatus {
                status: super::CalTodoStatus::InProcess,
                percent: Some(40),
                completed: None,
            });
            let item = request.execute(&mut state).unwrap();
            let file = state.store().file_by_id(&item.uid).unwrap();
            let task = file.components()[0].as_todo().unwrap();
            assert_eq!(task.start().is_some(), has_start);
            assert_eq!(task.due().is_some(), has_due);
            assert_eq!(task.percent(), Some(40));
            let disk = fs::read_to_string(file.path()).unwrap();
            assert!(disk.contains("BEGIN:VTODO"));
            assert_eq!(disk.contains("DUE;VALUE=DATE:20261015"), has_due);
            assert!(!disk.contains("DTEND"));
            assert_eq!(
                state.misc().last_calendar(CalCompType::Todo),
                Some(&"test".into())
            );
        }
    }

    #[test]
    fn recurring_tasks_require_a_start_and_events_require_both_dates() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path(), false);
        let mut task = command();
        task.kind = CalCompType::Todo;
        task.start = None;
        task.recurrence = Some("FREQ=DAILY;COUNT=3".parse().unwrap());
        assert!(task.execute(&mut state).is_err());
        for start in [true, false] {
            let mut event = command();
            if start {
                event.start = None;
            } else {
                event.end = None;
            }
            assert!(event.execute(&mut state).is_err());
        }
        assert_eq!(state.store().files().count(), 0);
    }

    #[test]
    fn creates_self_contained_recurring_event_with_alarm_and_attendee() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path(), false);
        let mut request = command();
        request.recurrence = Some("FREQ=WEEKLY;COUNT=3;BYDAY=TH".parse().unwrap());
        let alarm = CalAlarm::new(
            eventix_ical::objects::CalAction::Display,
            eventix_ical::objects::CalTrigger::Relative {
                related: eventix_ical::objects::CalRelated::Start,
                duration: chrono::Duration::minutes(-15).into(),
            },
        );
        request.alarms.push(alarm);
        let mut attendee = CalAttendee::new("mailto:guest@example.org".into());
        attendee.set_common_name("Guest".into());
        attendee.set_role(CalRole::Optional);
        request.attendees.push(attendee);
        let identity = request.execute(&mut state).unwrap();
        let path = tmp.path().join(format!("{}.ics", identity.uid));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("BEGIN:VTIMEZONE"));
        assert!(content.contains("TZID:Europe/Berlin"));
        let loaded = CalDir::new_from_dir(
            Arc::new("test".into()),
            tmp.path().into(),
            "Test".into(),
            false,
        )
        .unwrap();
        let occurrence = loaded
            .occurrence_by_id(&identity.uid, None, &chrono_tz::UTC)
            .unwrap();
        assert_eq!(occurrence.summary().unwrap(), "New event");
        assert_eq!(occurrence.location().unwrap(), "Office");
        assert_eq!(
            occurrence.description().unwrap(),
            "Some details\nSecond line"
        );
        assert_eq!(occurrence.rrule().unwrap().count(), Some(3));
        assert_eq!(occurrence.alarms().unwrap().len(), 1);
        assert_eq!(
            occurrence.attendees().unwrap()[0].role(),
            Some(CalRole::Optional)
        );
        assert_eq!(
            state.misc().last_calendar(CalCompType::Event),
            Some(&"test".into())
        );
        assert!(
            state
                .store()
                .occurrence_by_id(&identity.uid, None, state.timezone())
                .is_some()
        );
    }

    #[test]
    fn all_day_end_is_persisted_as_exclusive() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path(), false);
        let mut request = command();
        request.start = Some(CalDate::new_date(
            "2026-10-15".parse().unwrap(),
            CalDateType::Inclusive,
        ));
        request.end = Some(CalDate::new_date(
            "2026-10-16".parse().unwrap(),
            CalDateType::Exclusive,
        ));
        let identity = request.execute(&mut state).unwrap();
        let content = fs::read_to_string(tmp.path().join(format!("{}.ics", identity.uid))).unwrap();
        assert!(content.contains("DTSTART;VALUE=DATE:20261015"));
        assert!(content.contains("DTEND;VALUE=DATE:20261016"));
    }

    #[test]
    fn rejects_invalid_or_stale_input_without_creating_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path(), false);
        for case in 0..6 {
            let mut request = command();
            match case {
                0 => request.summary = "  ".into(),
                1 => request.calendar = "missing".into(),
                2 => request.end = Some("TTEurope/Berlin;2026-10-14T11:00:00".parse().unwrap()),
                3 => {
                    request.end = Some(CalDate::new_date(
                        "2026-10-16".parse().unwrap(),
                        CalDateType::Exclusive,
                    ))
                }
                4 => request.start = Some("TTInvalid/Zone;2026-10-15T10:00:00".parse().unwrap()),
                _ => {
                    let mut rule = CalRRule::default();
                    rule.set_interval(0);
                    request.recurrence = Some(rule);
                }
            }
            assert!(request.execute(&mut state).is_err(), "case {case}");
        }
        assert_eq!(state.store().files().count(), 0);
        assert!(state.misc().last_calendar(CalCompType::Event).is_none());
        assert!(fs::read_dir(tmp.path()).unwrap().all(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_none_or(|ext| ext != "ics")
        }));
    }

    #[test]
    fn rejects_write_protected_directories_and_unsupported_types() {
        let tmp = tempfile::tempdir().unwrap();
        let mut protected = state(tmp.path(), true);
        assert!(command().execute(&mut protected).is_err());
        let mut state = state(tmp.path(), false);
        state
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .all_calendars_mut()
            .get_mut("test")
            .unwrap()
            .set_types(vec![CalCompType::Todo]);
        assert!(command().execute(&mut state).is_err());
        assert_eq!(state.store().files().count(), 0);
    }
}
