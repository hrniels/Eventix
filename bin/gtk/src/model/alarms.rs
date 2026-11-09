// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Due alarm collection and localized notification content, independent of GTK.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use eventix_ical::{
    col::AlarmOccurrence,
    objects::{AlarmOverlay, CalAlarm, CalComponent, CalDate, DefaultAlarmOverlay, EventLike},
};
use eventix_locale::{DateFlags, Locale};
use eventix_state::{CalendarAlarmType, PersonalCalendarAlarms, State};

pub(crate) struct Notification {
    pub summary: String,
    pub body: String,
}

impl Notification {
    fn new(alarm: &AlarmOccurrence<'_>, locale: &dyn Locale) -> Self {
        let occurrence = alarm.occurrence();
        let mut lines = Vec::new();
        if let Some(start) = occurrence.occurrence_start() {
            lines.push(format!(
                "{}: {}",
                locale.translate("Start"),
                locale.fmt_datetime(&start, DateFlags::None)
            ));
        }
        if let Some(location) = occurrence.location() {
            lines.push(format!("{}: {location}", locale.translate("Location")));
        }
        if let Some(date) = alarm.alarm_date() {
            lines.push(format!(
                "{}: {}",
                locale.translate("Reminder"),
                locale.fmt_datetime(&date, DateFlags::Short)
            ));
        }
        Self {
            summary: occurrence
                .summary()
                .filter(|summary| !summary.is_empty())
                .cloned()
                .unwrap_or_else(|| locale.translate("Reminder").into()),
            body: lines.join("\n"),
        }
    }
}

struct PersonalOverlay<'a> {
    personal: Option<&'a PersonalCalendarAlarms>,
    default: &'a Option<CalAlarm>,
}

impl AlarmOverlay for PersonalOverlay<'_> {
    fn alarms_for_component(&self, component: &CalComponent) -> Option<Vec<CalAlarm>> {
        self.personal
            .and_then(|personal| personal.get(component.uid(), None))
            .map(|overwrite| overwrite.alarms().to_vec())
            .or_else(|| self.default.clone().map(|alarm| vec![alarm]))
    }

    fn alarm_overwrites(
        &self,
        component: &CalComponent,
        overwrites: HashMap<CalDate, &[CalAlarm]>,
    ) -> HashMap<CalDate, Vec<CalAlarm>> {
        if let Some(personal) = self.personal {
            let mut personal = personal.all_for_occurrences(component.uid());
            for (date, alarms) in overwrites {
                personal.entry(date).or_insert_with(|| alarms.to_vec());
            }
            personal
        } else {
            HashMap::default()
        }
    }
}

/// Collects due reminders in time order and records the check time, logging persistence failures.
pub(crate) fn check(state: &mut State, now: DateTime<Utc>) -> Vec<Notification> {
    let start = state.misc().last_alarm_check().and_utc();
    // A backwards clock adjustment must not replay already delivered reminders.
    if now <= start {
        return Vec::new();
    }
    let notifications = {
        let locale = state.locale();
        let start = start.with_timezone(locale.timezone());
        let end = now.with_timezone(locale.timezone());
        let mut alarms = state
            .store()
            .directories()
            .iter()
            .flat_map(|directory| {
                let Some((_, settings)) = state.settings().calendar(directory.id()) else {
                    return Vec::new();
                };
                let overlay: Box<dyn AlarmOverlay> = match settings.alarms() {
                    CalendarAlarmType::Calendar => Box::new(DefaultAlarmOverlay),
                    CalendarAlarmType::Personal { default } => Box::new(PersonalOverlay {
                        personal: state.personal_alarms().get(directory.id()),
                        default,
                    }),
                };
                directory
                    .due_alarms_between(start, end, &*overlay)
                    .filter(|alarm| !alarm.occurrence().is_cancelled())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        alarms.sort_by_key(|alarm| alarm.alarm_date());
        alarms
            .iter()
            .map(|alarm| Notification::new(alarm, locale.as_ref()))
            .collect()
    };
    let misc = state.misc_mut();
    misc.set_last_alarm_check(now.naive_utc());
    if let Err(error) = misc.write_to_file() {
        tracing::warn!(%error, "Unable to save last alarm check");
    }
    notifications
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{fs, sync::Arc};

    use eventix_ical::{
        col::CalDir,
        objects::{CalAction, CalTrigger},
    };
    use eventix_state::{CalendarSettings, CollectionSettings, SyncerType};

    fn date(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    fn alarm() -> CalAlarm {
        CalAlarm::new(
            CalAction::Display,
            CalTrigger::Relative {
                related: eventix_ical::objects::CalRelated::Start,
                duration: "-PT15M".parse().unwrap(),
            },
        )
    }

    fn state(directory: &std::path::Path, components: &str) -> State {
        fs::write(
            directory.join("test.ics"),
            format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{components}END:VCALENDAR\r\n"),
        )
        .unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(directory, directory));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        state.store_mut().add(
            CalDir::new_from_dir(
                Arc::new("test".into()),
                directory.into(),
                "Test".into(),
                false,
            )
            .unwrap(),
        );
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: directory.display().to_string(),
        });
        let mut calendar = CalendarSettings::default();
        calendar.set_enabled(true);
        calendar.set_alarms(CalendarAlarmType::Calendar);
        collection
            .all_calendars_mut()
            .insert("test".into(), calendar);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        state
            .misc_mut()
            .set_last_alarm_check(date("2026-10-12T08:00:00Z").naive_utc());
        state
    }

    fn event(uid: &str, properties: &str) -> String {
        format!(
            "BEGIN:VEVENT\r\nUID:{uid}\r\nSUMMARY:{uid}\r\n\
             DTSTART:20261012T090000Z\r\nDTEND:20261012T100000Z\r\n\
             LOCATION:Office\r\n{properties}\
             BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\n\
             DESCRIPTION:Reminder\r\nEND:VALARM\r\nEND:VEVENT\r\n"
        )
    }

    #[test]
    fn delivers_once_across_check_boundaries_and_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path(), &event("Meeting", ""));
        let boundary = date("2026-10-12T08:45:00Z");
        assert!(check(&mut state, boundary).is_empty());
        let notifications = check(&mut state, boundary + chrono::Duration::seconds(30));
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].summary, "Meeting");
        assert!(notifications[0].body.contains("Location: Office"));
        assert!(notifications[0].body.contains("Start: "));
        assert!(notifications[0].body.contains("Reminder: "));
        assert!(check(&mut state, date("2026-10-12T08:46:00Z")).is_empty());
        let xdg = Arc::new(state.xdg().clone());
        state.settings().write_to_file().unwrap();
        let mut restarted = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        assert!(check(&mut restarted, date("2026-10-12T08:47:00Z")).is_empty());
        assert!(check(&mut restarted, boundary).is_empty());
        assert_eq!(
            restarted.misc().last_alarm_check(),
            date("2026-10-12T08:47:00Z").naive_utc()
        );
    }

    #[test]
    fn recurring_reminders_skip_cancelled_and_excluded_events() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("cancelled.ics"),
            format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{}END:VCALENDAR\r\n",
                event("Cancelled", "STATUS:CANCELLED\r\n")
            ),
        )
        .unwrap();
        let components = event(
            "Recurring",
            "RRULE:FREQ=DAILY;COUNT=3\r\nEXDATE:20261013T090000Z\r\n",
        );
        let mut state = state(tmp.path(), &components);
        let notifications = check(&mut state, date("2026-10-15T00:00:00Z"));
        assert_eq!(notifications.len(), 2);
        assert!(
            notifications
                .iter()
                .all(|notification| notification.summary == "Recurring")
        );
    }

    #[test]
    fn task_and_event_reminders_are_sorted_and_localized() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("task.ics"),
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
               BEGIN:VTODO\r\nUID:task\r\nSUMMARY:Task\r\n\
               DTSTART:20261012T083000Z\r\nDUE:20261012T090000Z\r\n\
               BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\n\
               DESCRIPTION:Reminder\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let mut state = state(tmp.path(), &event("Meeting", ""));
        let mut preferences = state.settings().preferences();
        preferences.locale_type = eventix_locale::LocaleType::German;
        state.save_preferences(preferences).unwrap();
        let notifications = check(&mut state, date("2026-10-12T09:00:00Z"));
        assert_eq!(notifications.len(), 2);
        assert_eq!(notifications[0].summary, "Task");
        assert_eq!(notifications[1].summary, "Meeting");
        assert!(notifications[1].body.contains("Ort: Office"));
        assert!(notifications[1].body.contains("Erinnerung: "));
    }

    #[test]
    fn personal_defaults_and_empty_series_and_occurrence_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(
            tmp.path(),
            &event("Recurring", "RRULE:FREQ=DAILY;COUNT=3\r\n"),
        );
        state
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .all_calendars_mut()
            .get_mut("test")
            .unwrap()
            .set_alarms(CalendarAlarmType::Personal {
                default: Some(alarm()),
            });
        let personal = state.personal_alarms_mut().get_or_create("test");
        personal.set(
            "Recurring",
            Some(&"TU2026-10-13T09:00:00".parse().unwrap()),
            vec![],
        );
        assert_eq!(check(&mut state, date("2026-10-15T00:00:00Z")).len(), 2);

        state
            .misc_mut()
            .set_last_alarm_check(date("2026-10-12T08:00:00Z").naive_utc());
        state
            .personal_alarms_mut()
            .get_or_create("test")
            .set("Recurring", None, vec![]);
        assert!(check(&mut state, date("2026-10-15T00:00:00Z")).is_empty());
    }
}
