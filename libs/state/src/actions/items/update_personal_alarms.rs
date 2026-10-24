// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use anyhow::{Context, bail};
use eventix_ical::objects::CalAlarm;

use super::ItemId;
use crate::{CalendarAlarmType, State, actions::Action};

/// Updates a personal reminder override, or restores inherited reminders with `None`.
#[derive(Debug)]
pub struct UpdatePersonalAlarms {
    pub item: ItemId,
    pub alarms: Option<Vec<CalAlarm>>,
}

impl Action for UpdatePersonalAlarms {
    fn affected_items(&self) -> &[ItemId] {
        std::slice::from_ref(&self.item)
    }

    fn execute(self: Box<Self>, state: &mut State) -> anyhow::Result<ItemId> {
        let locale = state.locale();
        let (_, calendar) = state.settings().calendar(&self.item.calendar).context(
            locale
                .translate("Calendar is no longer available")
                .to_string(),
        )?;
        if !matches!(calendar.alarms(), CalendarAlarmType::Personal { .. }) {
            bail!(locale.translate("Could not apply changes").to_string());
        }
        state
            .store()
            .directory(&Arc::new(self.item.calendar.clone()))
            .and_then(|dir| dir.file_by_id(&self.item.uid))
            .and_then(|file| {
                file.occurrence_by_id(
                    &self.item.uid,
                    self.item.recurrence.as_ref(),
                    state.timezone(),
                )
            })
            .context(
                locale
                    .translate("Occurrence is no longer available")
                    .to_string(),
            )?;

        let personal = state
            .personal_alarms_mut()
            .get_or_create(&self.item.calendar);
        // Persist a staged copy so a failed write leaves the active reminders intact.
        let mut staged = personal.clone();
        let changed = match self.alarms {
            Some(alarms) => staged.set(&self.item.uid, self.item.recurrence.as_ref(), alarms),
            None => staged.unset(&self.item.uid, self.item.recurrence.as_ref()),
        };
        if changed {
            staged.save()?;
            *personal = staged;
        }
        Ok(self.item)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use chrono::Duration;
    use eventix_ical::{
        col::CalDir,
        objects::{CalAction, CalAlarm, CalRelated, CalTrigger},
    };

    use super::{Action, ItemId, UpdatePersonalAlarms};
    use crate::{
        CalendarAlarmType, CalendarSettings, CollectionSettings, PersonalAlarms, State, SyncerType,
    };

    fn fixture(path: &std::path::Path) -> State {
        fs::write(
            path.join("event.ics"),
            concat!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:event\r\n",
                "DTSTART:20261012T090000Z\r\nDTEND:20261012T100000Z\r\n",
                "RRULE:FREQ=DAILY;COUNT=3\r\nORGANIZER:mailto:other@example.org\r\n",
                "END:VEVENT\r\nEND:VCALENDAR\r\n",
            ),
        )
        .unwrap();
        let xdg = Arc::new(crate::with_test_xdg(path, path));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        state.store_mut().add(
            CalDir::new_from_dir(Arc::new("test".into()), path.into(), "Test".into(), false)
                .unwrap(),
        );
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: path.to_string_lossy().into_owned(),
        });
        let mut calendar = CalendarSettings::default();
        calendar.set_enabled(true);
        calendar.set_alarms(CalendarAlarmType::Personal { default: None });
        collection
            .all_calendars_mut()
            .insert("test".into(), calendar);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        state
    }

    fn command(recurrence: bool, alarms: Option<Vec<CalAlarm>>) -> Box<UpdatePersonalAlarms> {
        Box::new(UpdatePersonalAlarms {
            item: ItemId {
                calendar: "test".into(),
                uid: "event".into(),
                recurrence: recurrence.then(|| "TU2026-10-13T09:00:00".parse().unwrap()),
            },
            alarms,
        })
    }

    fn alarm() -> CalAlarm {
        CalAlarm::new(
            CalAction::Display,
            CalTrigger::Relative {
                duration: Duration::minutes(-15).into(),
                related: CalRelated::Start,
            },
        )
    }

    #[test]
    fn saves_series_and_occurrence_overrides_and_restores_inheritance() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path());
        let original = fs::read(tmp.path().join("event.ics")).unwrap();
        command(false, Some(vec![alarm()]))
            .execute(&mut state)
            .unwrap();
        command(true, Some(vec![])).execute(&mut state).unwrap();
        let rid = command(true, None).item.recurrence;
        let persisted =
            PersonalAlarms::new_from_dir(&crate::with_test_xdg(tmp.path(), tmp.path())).unwrap();
        let personal = persisted.get("test").unwrap();
        assert_eq!(personal.get("event", None).unwrap().alarms(), &[alarm()]);
        assert!(
            personal
                .get("event", rid.as_ref())
                .unwrap()
                .alarms()
                .is_empty()
        );

        command(true, None).execute(&mut state).unwrap();
        let personal = state.personal_alarms().get("test").unwrap();
        assert_eq!(
            personal.get("event", rid.as_ref()).unwrap().alarms(),
            &[alarm()]
        );
        command(false, None).execute(&mut state).unwrap();
        assert!(
            state
                .personal_alarms()
                .get("test")
                .unwrap()
                .get("event", None)
                .is_none()
        );
        assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), original);
    }

    #[test]
    fn failed_persistence_preserves_active_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path());
        command(false, Some(vec![alarm()]))
            .execute(&mut state)
            .unwrap();
        let path = tmp.path().join("alarms/test.toml");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(command(false, Some(vec![])).execute(&mut state).is_err());
        assert_eq!(
            state
                .personal_alarms()
                .get("test")
                .unwrap()
                .get("event", None)
                .unwrap()
                .alarms(),
            &[alarm()]
        );
    }

    #[test]
    fn rejects_stale_identity_and_calendars_without_personal_alarms() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path());
        let mut stale = command(false, Some(vec![]));
        stale.item.uid = "missing".into();
        assert!(stale.execute(&mut state).is_err());
        state
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .all_calendars_mut()
            .get_mut("test")
            .unwrap()
            .set_alarms(CalendarAlarmType::Calendar);
        assert!(command(false, Some(vec![])).execute(&mut state).is_err());
        assert!(state.personal_alarms().get("test").is_none());
    }
}
