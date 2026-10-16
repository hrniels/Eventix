// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::bail;
use eventix_ical::objects::{
    CalAlarm, CalAttendee, CalCompType, CalDate, CalDateTime, CalRRule, DateContext,
    UpdatableEventLike,
};

use super::{ItemId, create_component};
use crate::{CalendarAlarmType, State};

/// Creates an event from frontend-independent, already parsed field values.
#[derive(Debug)]
pub struct CreateEvent {
    pub calendar: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub start: CalDate,
    pub end: CalDate,
    pub recurrence: Option<CalRRule>,
    pub alarms: Vec<CalAlarm>,
    pub personal_alarms: Option<Vec<CalAlarm>>,
    pub attendees: Vec<CalAttendee>,
}

impl CreateEvent {
    /// Validates and persists an event, returning its new identity.
    pub fn execute(self, state: &mut State) -> anyhow::Result<ItemId> {
        let locale = state.locale();

        if self.summary.trim().is_empty() {
            bail!(locale.translate("error.summary_empty").to_string());
        }

        if matches!(self.start, CalDate::Date(..)) != matches!(self.end, CalDate::Date(..)) {
            bail!(locale.translate("error.time_for_both_or_none").to_string());
        }

        for date in [&self.start, &self.end] {
            if let CalDate::DateTime(CalDateTime::Timezone(_, name)) = date {
                name.parse::<chrono_tz::Tz>().map_err(|_| {
                    anyhow::anyhow!(locale.translate("error.valid_date_time").to_string())
                })?;
            }
        }

        let context = DateContext::system();
        if context.date_to_utc(&self.start, state.timezone())
            > context.date_to_utc(&self.end, state.timezone())
        {
            bail!(locale.translate("error.end_before_start").to_string());
        }

        if let Some(rule) = &self.recurrence {
            if rule.interval() == Some(0) || rule.count() == Some(0) {
                bail!(locale.translate("Invalid recurrence").to_string());
            }

            if let Some(until) = rule.until()
                && context.date_to_utc(until, state.timezone())
                    < context.date_to_utc(&self.start, state.timezone())
            {
                bail!(locale.translate("error.end_before_start").to_string());
            }
        }

        let personal_alarms = self.personal_alarms;
        let identity = create_component(
            state,
            &locale,
            &self.calendar,
            CalCompType::Event,
            |_, _, component, _, organizer, _, _| {
                component.set_summary(Some(self.summary));
                component.set_location((!self.location.is_empty()).then_some(self.location));
                component
                    .set_description((!self.description.is_empty()).then_some(self.description));
                component.set_start(Some(self.start));
                if let Some(event) = component.as_event_mut() {
                    event.set_end(Some(self.end));
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

    use super::{CalAlarm, CalAttendee, CalDate, CalRRule, CreateEvent, State};
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

    fn command() -> CreateEvent {
        CreateEvent {
            calendar: "test".into(),
            summary: "New event".into(),
            location: "Office".into(),
            description: "Some details\nSecond line".into(),
            start: "TTEurope/Berlin;2026-10-15T10:00:00".parse().unwrap(),
            end: "TTEurope/Berlin;2026-10-15T11:00:00".parse().unwrap(),
            recurrence: None,
            alarms: Vec::new(),
            personal_alarms: None,
            attendees: Vec::new(),
        }
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
        request.start = CalDate::new_date("2026-10-15".parse().unwrap(), CalDateType::Inclusive);
        request.end = CalDate::new_date("2026-10-16".parse().unwrap(), CalDateType::Exclusive);
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
                2 => request.end = "TTEurope/Berlin;2026-10-14T11:00:00".parse().unwrap(),
                3 => {
                    request.end =
                        CalDate::new_date("2026-10-16".parse().unwrap(), CalDateType::Exclusive)
                }
                4 => request.start = "TTInvalid/Zone;2026-10-15T10:00:00".parse().unwrap(),
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
