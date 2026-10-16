// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fs, io::Write, sync::Arc};

use anyhow::{Context, bail};
use chrono::{DateTime, NaiveDateTime, Utc};
use eventix_ical::{
    col::CalFile,
    objects::{CalDate, CalDateTime, EventLike, UpdatableEventLike},
};

use super::ItemId;
use crate::State;

/// Moves one timed occurrence to a display-local time, preserving elapsed duration.
///
/// Recurring items retain their original recurrence identity. DST gaps and folds follow
/// `CalEvent::shift_to` semantics. Expected bounds reject stale moves. The calendar is staged
/// and atomically replaced on disk before the in-memory calendar is changed.
#[derive(Debug)]
pub struct MoveEvent {
    pub item: ItemId,
    pub start: NaiveDateTime,
    pub expected_start: DateTime<Utc>,
    pub expected_end: DateTime<Utc>,
}

impl MoveEvent {
    /// Persists the move and returns the identity to invalidate after a successful commit.
    pub fn execute(self, state: &mut State) -> anyhow::Result<ItemId> {
        let locale = state.locale();
        let tz = *state.timezone();

        let directory = Arc::new(self.item.calendar.clone());
        let (collection, _) = state
            .settings()
            .calendar(&self.item.calendar)
            .with_context(|| {
                locale
                    .translate("Calendar is no longer available")
                    .to_string()
            })?;
        let user_mail = collection.email().map(|email| email.address().clone());

        let file = state
            .store_mut()
            .directory_mut(&directory)?
            .file_by_id_mut(&self.item.uid)?;
        let occurrence = file
            .occurrence_by_id(&self.item.uid, self.item.recurrence.as_ref(), &tz)
            .with_context(|| {
                locale
                    .translate("Occurrence is no longer available")
                    .to_string()
            })?;

        if occurrence.occurrence_start().map(|d| d.with_timezone(&Utc)) != Some(self.expected_start)
            || occurrence.occurrence_end().map(|d| d.with_timezone(&Utc)) != Some(self.expected_end)
        {
            bail!(locale.translate("error.component_was_modified").to_string());
        }

        if occurrence.is_all_day() || occurrence.is_excluded() {
            bail!(
                locale
                    .translate("Only timed events can be moved here")
                    .to_string()
            );
        }

        // Staging isolates both callback errors and persistence failures from live state.
        let mut staged = CalFile::new(directory, file.path().clone(), file.calendar().clone());
        let ctx = staged.calendar().date_context();
        staged.change_single(
            &self.item.uid,
            self.item.recurrence.as_ref(),
            &tz,
            user_mail.as_ref(),
            true,
            |_, component| {
                let event = component
                    .as_event_mut()
                    .ok_or_else(|| locale.translate("Component is not an event").to_string())?;
                event
                    .shift_to(
                        &ctx,
                        CalDate::DateTime(CalDateTime::Timezone(self.start, tz.name().into())),
                        &tz,
                    )
                    .map_err(|error| error.to_string())?;
                component.touch();
                Ok(())
            },
        )?;

        let path = file.path();
        let parent = path.parent().with_context(|| {
            locale
                .translate("Calendar file has no parent directory")
                .to_string()
        })?;

        let mut output = tempfile::NamedTempFile::new_in(parent)?;
        output
            .as_file()
            .set_permissions(fs::metadata(path)?.permissions())?;
        staged.calendar().write(output.as_file_mut())?;
        output.flush()?;
        output.as_file().sync_all()?;
        output.persist(path)?;

        *file.calendar_mut() = staged.calendar().clone();

        Ok(self.item)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use chrono::{Duration, TimeZone, Timelike};
    use eventix_ical::col::{CalDir, CalFile};

    use super::{ItemId, MoveEvent};
    use crate::{CalendarSettings, CollectionSettings, State, SyncerType};

    fn fixture(path: &Path, recurrence: &str, read_only: bool) -> State {
        let calendar = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
            UID:move-me\r\nDTSTART:20261012T090000Z\r\nDTEND:20261012T100000Z\r\n\
            {recurrence}SUMMARY:Move me\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        );
        fs::write(path.join("event.ics"), calendar).unwrap();
        let xdg = Arc::new(crate::with_test_xdg(path, path));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        state.store_mut().add(
            CalDir::new_from_dir(
                Arc::new("test".into()),
                path.to_path_buf(),
                "Test".into(),
                read_only,
            )
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
        state
    }

    fn command(state: &State, recurrence: Option<eventix_ical::objects::CalDate>) -> MoveEvent {
        let occurrence = state
            .store()
            .occurrence_by_id("move-me", recurrence.as_ref(), state.timezone())
            .unwrap();
        MoveEvent {
            item: ItemId {
                calendar: "test".into(),
                uid: "move-me".into(),
                recurrence,
            },
            start: "2026-10-15T14:30:00".parse().unwrap(),
            expected_start: occurrence.occurrence_start().unwrap().to_utc(),
            expected_end: occurrence.occurrence_end().unwrap().to_utc(),
        }
    }

    #[test]
    fn moves_and_persists_only_the_target_calendar_and_rejects_stale_moves() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), "", false);
        let stale = command(&state, None);
        let mut other = CalDir::new_empty(
            Arc::new("other".into()),
            tmp.path().into(),
            "Other".into(),
            false,
        );
        other
            .add_file(CalFile::new(
                Arc::new("other".into()),
                tmp.path().join("other.ics"),
                state
                    .store()
                    .file_by_id("move-me")
                    .unwrap()
                    .calendar()
                    .clone(),
            ))
            .unwrap();
        state.store_mut().add(other);
        let item = command(&state, None).execute(&mut state).unwrap();
        let target = state.store().directory(&Arc::new(item.calendar)).unwrap();
        let occurrence = target
            .occurrence_by_id("move-me", None, state.timezone())
            .unwrap();
        let expected = chrono_tz::UTC
            .with_ymd_and_hms(2026, 10, 15, 14, 30, 0)
            .unwrap();
        assert_eq!(occurrence.occurrence_start(), Some(expected));
        assert_eq!(
            occurrence.occurrence_end(),
            Some(expected + Duration::hours(1))
        );
        let persisted =
            CalFile::new_from_file(Arc::new("test".into()), tmp.path().join("event.ics")).unwrap();
        let saved = persisted
            .occurrence_by_id("move-me", None, state.timezone())
            .unwrap();
        assert_eq!(saved.occurrence_start(), Some(expected));
        assert_eq!(saved.occurrence_end(), Some(expected + Duration::hours(1)));
        let other = state.store().directory(&Arc::new("other".into())).unwrap();
        assert_eq!(
            other
                .occurrence_by_id("move-me", None, state.timezone())
                .unwrap()
                .occurrence_start()
                .unwrap()
                .date_naive()
                .to_string(),
            "2026-10-12"
        );
        let before = fs::read(tmp.path().join("event.ics")).unwrap();
        assert!(stale.execute(&mut state).is_err());
        assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), before);
    }

    #[test]
    fn recurring_moves_keep_original_identity_and_leave_other_occurrences_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), "RRULE:FREQ=DAILY;COUNT=5\r\n", false);
        let rid = "TU2026-10-13T09:00:00".parse().unwrap();
        let item = command(&state, Some(rid)).execute(&mut state).unwrap();
        assert_eq!(
            item.recurrence.as_ref().unwrap().to_string(),
            "TU2026-10-13T09:00:00"
        );
        let occurrence = state
            .store()
            .occurrence_by_id(&item.uid, item.recurrence.as_ref(), state.timezone())
            .unwrap();
        assert!(occurrence.is_overwritten());
        assert_eq!(
            occurrence
                .occurrence_start()
                .unwrap()
                .naive_local()
                .to_string(),
            "2026-10-15 14:30:00"
        );
        let next_rid = "TU2026-10-14T09:00:00".parse().unwrap();
        let next = state
            .store()
            .occurrence_by_id(&item.uid, Some(&next_rid), state.timezone())
            .unwrap();
        assert!(!next.is_overwritten());
        assert_eq!(next.occurrence_start().unwrap().hour(), 9);
        command(&state, item.recurrence.clone())
            .execute(&mut state)
            .unwrap();
        assert_eq!(
            state
                .store()
                .file_by_id(&item.uid)
                .unwrap()
                .components()
                .len(),
            2
        );
    }

    #[test]
    fn failed_persistence_does_not_change_memory_or_replace_the_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), "", false);
        let request = command(&state, None);
        let before = state
            .store()
            .file_by_id("move-me")
            .unwrap()
            .calendar()
            .clone();
        // Force failure at the replacement, after writing the staged calendar.
        fs::remove_file(tmp.path().join("event.ics")).unwrap();
        fs::create_dir(tmp.path().join("event.ics")).unwrap();
        let entries = fs::read_dir(tmp.path()).unwrap().count();
        assert!(request.execute(&mut state).is_err());
        assert_eq!(
            state.store().file_by_id("move-me").unwrap().calendar(),
            &before
        );
        assert!(tmp.path().join("event.ics").is_dir());
        assert_eq!(
            fs::read_dir(tmp.path()).unwrap().count(),
            entries,
            "temporary file is cleaned up"
        );
    }

    #[test]
    fn read_only_and_sync_protected_calendars_reject_moves() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), "", true);
        let before = fs::read(tmp.path().join("event.ics")).unwrap();
        assert!(command(&state, None).execute(&mut state).is_err());
        assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), before);
        let mut state = fixture(tmp.path(), "", false);
        let protection = state
            .store_mut()
            .protect_directories([Arc::new("test".into())])
            .unwrap();
        assert!(command(&state, None).execute(&mut state).is_err());
        drop(protection);
        command(&state, None).execute(&mut state).unwrap();
    }
}
