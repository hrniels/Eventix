// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fs, io::Write, sync::Arc};

use anyhow::{Context, bail};
use chrono::{DateTime, NaiveDateTime, Utc};
use eventix_ical::{
    col::CalFile,
    objects::{CalDate, CalDateTime, EventLike, RangeEdge, UpdatableEventLike},
};

use super::ItemId;
use crate::State;

/// Moves one occurrence to a display-local time, preserving its span.
///
/// Recurring items retain their original recurrence identity. DST gaps and folds follow
/// `CalEvent::shift_to` semantics. Timed events preserve elapsed duration; all-day events preserve
/// their number of calendar days. Expected bounds reject stale moves. The calendar is staged
/// and atomically replaced on disk before the in-memory calendar is changed.
#[derive(Debug)]
pub struct MoveEvent {
    pub item: ItemId,
    pub start: NaiveDateTime,
    pub expected_start: DateTime<Utc>,
    pub expected_end: DateTime<Utc>,
}

/// Resizes one edge of a timed occurrence, rejecting stale bounds and invalid ranges.
#[derive(Debug)]
pub struct ResizeEvent {
    pub item: ItemId,
    pub edge: RangeEdge,
    pub value: NaiveDateTime,
    pub expected_start: DateTime<Utc>,
    pub expected_end: DateTime<Utc>,
}

impl ResizeEvent {
    /// Persists the resize and returns the identity to invalidate after a successful commit.
    pub fn execute(self, state: &mut State) -> anyhow::Result<ItemId> {
        MoveEvent {
            item: self.item,
            start: self.value,
            expected_start: self.expected_start,
            expected_end: self.expected_end,
        }
        .execute_change(state, Some(self.edge))
    }
}

impl MoveEvent {
    /// Persists the move and returns the identity to invalidate after a successful commit.
    pub fn execute(self, state: &mut State) -> anyhow::Result<ItemId> {
        self.execute_change(state, None)
    }

    fn execute_change(self, state: &mut State, edge: Option<RangeEdge>) -> anyhow::Result<ItemId> {
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

        if occurrence.is_excluded() || (edge.is_some() && occurrence.is_all_day()) {
            bail!(locale.translate("Could not apply changes").to_string());
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
                let value = CalDate::DateTime(CalDateTime::Timezone(self.start, tz.name().into()));
                if let Some(edge) = edge {
                    let instant = match edge {
                        RangeEdge::Start => value.as_start_with_resolver(&tz, ctx.resolver()),
                        RangeEdge::End => value.as_end_with_resolver(&tz, ctx.resolver()),
                    };
                    let valid = match edge {
                        RangeEdge::Start => instant.with_timezone(&Utc) < self.expected_end,
                        RangeEdge::End => instant.with_timezone(&Utc) > self.expected_start,
                    };
                    if !valid {
                        return Err(locale.translate("Could not apply changes").to_string());
                    }
                    event.resize(&ctx, edge, value, &tz)
                } else {
                    event.shift_to(&ctx, value, &tz)
                }
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
    use eventix_ical::objects::{EventLike, RangeEdge};

    use super::{ItemId, MoveEvent, ResizeEvent};
    use crate::actions::items::CopyEvent;
    use crate::{CalendarSettings, CollectionSettings, State, SyncerType};

    fn fixture(path: &Path, recurrence: &str, read_only: bool) -> State {
        fixture_with_dates(
            path,
            recurrence,
            read_only,
            "DTSTART:20261012T090000Z\r\nDTEND:20261012T100000Z\r\n",
        )
    }

    fn fixture_with_dates(path: &Path, recurrence: &str, read_only: bool, dates: &str) -> State {
        let calendar = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
            UID:move-me\r\n{dates}\
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
        settings.set_types(vec![eventix_ical::objects::CalCompType::Event]);
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

    fn copy(request: MoveEvent) -> CopyEvent {
        CopyEvent {
            item: request.item,
            start: request.start,
            expected_start: request.expected_start,
            expected_end: request.expected_end,
        }
    }

    #[test]
    fn copying_preserves_source_details_and_span_in_memory_and_on_disk() {
        for dates in [
            "DTSTART:20261012T090000Z\r\nDTEND:20261012T100000Z\r\n",
            "DTSTART;VALUE=DATE:20261012\r\nDTEND;VALUE=DATE:20261014\r\n",
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = fixture_with_dates(tmp.path(), "", false, dates);
            let original = fs::read(tmp.path().join("event.ics")).unwrap();
            let request = command(&state, None);
            let span = request.expected_end - request.expected_start;
            let item = copy(request).execute(&mut state).unwrap();
            assert_ne!(item.uid, "move-me");
            assert!(item.recurrence.is_none());
            assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), original);
            let disk = CalFile::new_from_file(
                Arc::new("test".into()),
                tmp.path().join(format!("{}.ics", item.uid)),
            )
            .unwrap();
            for file in [state.store().file_by_id(&item.uid).unwrap(), &disk] {
                let occurrence = file
                    .occurrence_by_id(&item.uid, None, state.timezone())
                    .unwrap();
                assert_eq!(occurrence.summary().map(String::as_str), Some("Move me"));
                let start = occurrence.occurrence_start().unwrap();
                let end = occurrence.occurrence_end().unwrap();
                assert_eq!(start.date_naive().to_string(), "2026-10-15");
                assert_eq!(end - start, span);
                if !occurrence.is_all_day() {
                    assert_eq!(start.time().to_string(), "14:30:00");
                }
            }
        }
    }

    #[test]
    fn copying_rejects_stale_read_only_and_recurring_events_without_creating_files() {
        for (recurrence, read_only, stale) in [
            ("", false, true),
            ("", true, false),
            ("RRULE:FREQ=DAILY;COUNT=5\r\n", false, false),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = fixture(tmp.path(), recurrence, read_only);
            let mut request = copy(command(&state, None));
            if stale {
                request.expected_start += Duration::minutes(1);
            }
            let original = fs::read(tmp.path().join("event.ics")).unwrap();
            let files = fs::read_dir(tmp.path()).unwrap().count();
            assert!(request.execute(&mut state).is_err());
            assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), files);
            assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), original);
        }
    }

    #[test]
    fn all_day_moves_preserve_date_values_and_span_and_still_reject_resizing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture_with_dates(
            tmp.path(),
            "",
            false,
            "DTSTART;VALUE=DATE:20261012\r\nDTEND;VALUE=DATE:20261014\r\n",
        );
        assert!(
            resize(&state, None, RangeEdge::End, "2026-10-16T00:00:00")
                .execute(&mut state)
                .is_err()
        );
        command(&state, None).execute(&mut state).unwrap();
        let disk =
            CalFile::new_from_file(Arc::new("test".into()), tmp.path().join("event.ics")).unwrap();
        let occurrence = disk
            .occurrence_by_id("move-me", None, state.timezone())
            .unwrap();
        assert!(occurrence.is_all_day());
        assert_eq!(
            occurrence
                .occurrence_start()
                .unwrap()
                .date_naive()
                .to_string(),
            "2026-10-15"
        );
        assert_eq!(
            occurrence
                .occurrence_end()
                .unwrap()
                .naive_local()
                .to_string(),
            "2026-10-16 23:59:59"
        );
    }

    #[test]
    fn recurring_all_day_moves_preserve_span_in_memory_and_on_disk() {
        for (dates, days) in [
            (
                "DTSTART;VALUE=DATE:20261012\r\nDTEND;VALUE=DATE:20261013\r\n",
                1,
            ),
            (
                "DTSTART;VALUE=DATE:20261012\r\nDTEND;VALUE=DATE:20261015\r\n",
                3,
            ),
            ("DTSTART;VALUE=DATE:20261012\r\nDURATION:P1D\r\n", 1),
        ] {
            for destination in ["2026-10-13T00:00:00", "2026-10-24T00:00:00"] {
                let tmp = tempfile::tempdir().unwrap();
                let mut state =
                    fixture_with_dates(tmp.path(), "RRULE:FREQ=DAILY;COUNT=5\r\n", false, dates);
                let rid = "D2026-10-13;Exclusive".parse().unwrap();
                let mut request = command(&state, Some(rid));
                request.start = destination.parse().unwrap();
                let start = request.start;
                let item = request.execute(&mut state).unwrap();
                let disk =
                    CalFile::new_from_file(Arc::new("test".into()), tmp.path().join("event.ics"))
                        .unwrap();
                for file in [state.store().file_by_id(&item.uid).unwrap(), &disk] {
                    let occurrence = file
                        .occurrence_by_id(&item.uid, item.recurrence.as_ref(), state.timezone())
                        .unwrap();
                    assert!(occurrence.is_all_day());
                    assert!(occurrence.is_overwritten());
                    assert_eq!(occurrence.occurrence_start().unwrap().naive_local(), start);
                    assert_eq!(
                        occurrence.occurrence_end().unwrap().naive_local(),
                        start + Duration::days(days) - Duration::seconds(1),
                    );
                    let next_rid = "D2026-10-14;Exclusive".parse().unwrap();
                    let next = file
                        .occurrence_by_id(&item.uid, Some(&next_rid), state.timezone())
                        .unwrap();
                    assert!(!next.is_overwritten());
                    assert_eq!(
                        next.occurrence_start().unwrap().date_naive().to_string(),
                        "2026-10-14",
                    );
                }
            }
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

    fn resize(
        state: &State,
        recurrence: Option<eventix_ical::objects::CalDate>,
        edge: RangeEdge,
        value: &str,
    ) -> ResizeEvent {
        let original = command(state, recurrence);
        ResizeEvent {
            item: original.item,
            edge,
            value: value.parse().unwrap(),
            expected_start: original.expected_start,
            expected_end: original.expected_end,
        }
    }

    #[test]
    fn resizing_persists_each_edge_and_rejects_stale_or_inverted_ranges() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), "", false);
        let stale = resize(&state, None, RangeEdge::End, "2026-10-12T10:15:00");
        resize(&state, None, RangeEdge::Start, "2026-10-12T09:15:00")
            .execute(&mut state)
            .unwrap();
        let before = fs::read(tmp.path().join("event.ics")).unwrap();
        assert!(stale.execute(&mut state).is_err());
        for (edge, value) in [
            (RangeEdge::Start, "2026-10-12T10:00:00"),
            (RangeEdge::End, "2026-10-12T09:00:00"),
        ] {
            assert!(
                resize(&state, None, edge, value)
                    .execute(&mut state)
                    .is_err()
            );
        }
        assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), before);
        resize(&state, None, RangeEdge::End, "2026-10-13T00:00:00")
            .execute(&mut state)
            .unwrap();
        let disk =
            CalFile::new_from_file(Arc::new("test".into()), tmp.path().join("event.ics")).unwrap();
        let occurrence = disk
            .occurrence_by_id("move-me", None, state.timezone())
            .unwrap();
        assert_eq!(
            occurrence
                .occurrence_start()
                .unwrap()
                .naive_local()
                .to_string(),
            "2026-10-12 09:15:00"
        );
        assert_eq!(
            occurrence
                .occurrence_end()
                .unwrap()
                .naive_local()
                .to_string(),
            "2026-10-13 00:00:00"
        );
    }

    #[test]
    fn resizing_recurring_events_changes_only_the_selected_occurrence() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), "RRULE:FREQ=DAILY;COUNT=5\r\n", false);
        let rid = "TU2026-10-13T09:00:00".parse().unwrap();
        let item = resize(&state, Some(rid), RangeEdge::End, "2026-10-13T10:15:00")
            .execute(&mut state)
            .unwrap();
        let disk =
            CalFile::new_from_file(Arc::new("test".into()), tmp.path().join("event.ics")).unwrap();
        let changed = disk
            .occurrence_by_id(&item.uid, item.recurrence.as_ref(), state.timezone())
            .unwrap();
        assert!(changed.is_overwritten());
        assert_eq!(
            changed.occurrence_end().unwrap().naive_local().to_string(),
            "2026-10-13 10:15:00"
        );
        let next = "TU2026-10-14T09:00:00".parse().unwrap();
        let unchanged = disk
            .occurrence_by_id(&item.uid, Some(&next), state.timezone())
            .unwrap();
        assert!(!unchanged.is_overwritten());
        assert_eq!(unchanged.occurrence_end().unwrap().hour(), 10);
    }

    #[test]
    fn read_only_and_sync_protected_calendars_reject_resizing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), "", true);
        let before = fs::read(tmp.path().join("event.ics")).unwrap();
        assert!(
            resize(&state, None, RangeEdge::End, "2026-10-12T10:15:00")
                .execute(&mut state)
                .is_err()
        );
        assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), before);
        let mut state = fixture(tmp.path(), "", false);
        let protection = state
            .store_mut()
            .protect_directories([Arc::new("test".into())])
            .unwrap();
        assert!(
            resize(&state, None, RangeEdge::End, "2026-10-12T10:15:00")
                .execute(&mut state)
                .is_err()
        );
        drop(protection);
    }
}
