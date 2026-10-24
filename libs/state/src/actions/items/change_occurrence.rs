// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fs, io::Write, sync::Arc};

use anyhow::{Context, bail};
use eventix_ical::{
    col::CalFile,
    objects::{CalEventStatus, EventLike, UpdatableEventLike},
};
use formatx::formatx;

use super::ItemId;
use crate::{State, actions::Action};

/// Selects whether an occurrence is hidden or retained with canceled status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OccurrenceAction {
    Exclude,
    Cancel,
}

/// Excludes or cancels a single occurrence of an owned, writable recurring item.
#[derive(Debug)]
pub struct ChangeOccurrence {
    pub item: ItemId,
    pub action: OccurrenceAction,
}

impl Action for ChangeOccurrence {
    fn affected_items(&self) -> &[ItemId] {
        std::slice::from_ref(&self.item)
    }

    fn execute(self: Box<Self>, state: &mut State) -> anyhow::Result<ItemId> {
        let locale = state.locale();
        let timezone = *state.timezone();
        let recurrence = self
            .item
            .recurrence
            .as_ref()
            .context(locale.translate("Invalid recurrence").to_string())?;
        let (collection, _) = state.settings().calendar(&self.item.calendar).context(
            locale
                .translate("Calendar is no longer available")
                .to_string(),
        )?;
        let user_mail = collection.email().map(|email| email.address());
        if collection.is_read_only() {
            bail!(locale.translate("Could not apply changes").to_string());
        }

        let directory = Arc::new(self.item.calendar.clone());
        let file = state
            .store_mut()
            .directory_mut(&directory)?
            .file_by_id_mut(&self.item.uid)?;
        let occurrence = file
            .occurrence_by_id(&self.item.uid, Some(recurrence), &timezone)
            .context(
                locale
                    .translate("Occurrence is no longer available")
                    .to_string(),
            )?;
        if !occurrence.is_recurrent() || !occurrence.is_owned_by(user_mail.as_ref()) {
            bail!(locale.translate("Could not apply changes").to_string());
        }
        if self.action == OccurrenceAction::Cancel && occurrence.is_cancelled() {
            bail!(
                locale
                    .translate("Occurrence is already canceled")
                    .to_string()
            );
        }

        // Stage changes so validation and persistence failures leave live state untouched.
        let mut staged = CalFile::new(directory, file.path().clone(), file.calendar().clone());
        match self.action {
            OccurrenceAction::Exclude => {
                let start = staged
                    .component_with(|c| c.uid() == &self.item.uid && c.rid().is_none())
                    .and_then(|c| c.start().cloned())
                    .context(locale.translate("Invalid recurrence").to_string())?;
                let exdate = recurrence.normalize_to(&start, staged.calendar().timezone_resolver());
                let base = staged
                    .component_with_mut(|c| c.uid() == &self.item.uid && c.rid().is_none())
                    .context(
                        locale
                            .translate("Occurrence is no longer available")
                            .to_string(),
                    )?;
                base.toggle_exclude(exdate);
                base.touch();
            }
            OccurrenceAction::Cancel => {
                staged.change_single(
                    &self.item.uid,
                    Some(recurrence),
                    &timezone,
                    user_mail.as_ref(),
                    true,
                    |base, component| {
                        let summary = component
                            .summary()
                            .or_else(|| base.and_then(EventLike::summary));
                        let summary = summary.map(|summary| {
                            formatx!(locale.translate("Canceled: {}"), summary).unwrap()
                        });
                        let event = component.as_event_mut().ok_or_else(|| {
                            locale.translate("Component is not an event").to_string()
                        })?;
                        event.set_status(Some(CalEventStatus::Cancelled));
                        component.set_summary(summary);
                        component.touch();
                        Ok(())
                    },
                )?;
            }
        }

        let path = file.path();
        let parent = path.parent().context(
            locale
                .translate("Calendar file has no parent directory")
                .to_string(),
        )?;
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

    use eventix_ical::{
        col::CalDir,
        objects::{CalDate, CalEventStatus, EventLike},
    };

    use super::{Action, ChangeOccurrence, ItemId, OccurrenceAction};
    use crate::{CalendarSettings, CollectionSettings, EmailAccount, State, SyncerType};

    fn fixture(path: &Path, start: &str, organizer: &str, read_only: bool) -> State {
        fs::write(
            path.join("event.ics"),
            format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
            UID:meeting\r\n{start}\r\nRRULE:FREQ=DAILY;COUNT=5\r\n\
            SUMMARY:Meeting\r\n{organizer}END:VEVENT\r\nEND:VCALENDAR\r\n"
            ),
        )
        .unwrap();
        let xdg = Arc::new(crate::with_test_xdg(path, path));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        state.store_mut().add(
            CalDir::new_from_dir(
                Arc::new("test".into()),
                path.into(),
                "Test".into(),
                read_only,
            )
            .unwrap(),
        );
        let mut collection = CollectionSettings::new(SyncerType::VDirSyncer {
            email: EmailAccount::new("Host".into(), "host@example.org".into()),
            url: "https://example.org".into(),
            read_only,
            username: None,
            password: None,
            time_span: Default::default(),
        });
        let mut calendar = CalendarSettings::default();
        calendar.set_enabled(true);
        collection
            .all_calendars_mut()
            .insert("test".into(), calendar);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        state
    }

    fn command(recurrence: &str, action: OccurrenceAction) -> Box<ChangeOccurrence> {
        Box::new(ChangeOccurrence {
            item: ItemId {
                calendar: "test".into(),
                uid: "meeting".into(),
                recurrence: Some(recurrence.parse().unwrap()),
            },
            action,
        })
    }

    fn reload(path: &Path) -> CalDir {
        CalDir::new_from_dir(Arc::new("test".into()), path.into(), "Test".into(), false).unwrap()
    }

    #[test]
    fn exclusion_normalizes_dates_and_toggles_only_the_selected_occurrence() {
        for (start, recurrence, expected) in [
            (
                "DTSTART:20261012T090000Z",
                "TU2026-10-13T09:00:00",
                "EXDATE:20261013T090000Z",
            ),
            (
                "DTSTART;VALUE=DATE:20261012",
                "TU2026-10-13T00:00:00",
                "EXDATE;VALUE=DATE:20261013",
            ),
            (
                "DTSTART;TZID=Europe/Berlin:20261012T090000",
                "TU2026-10-13T07:00:00",
                "EXDATE;TZID=Europe/Berlin:20261013T090000",
            ),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = fixture(tmp.path(), start, "", false);
            command(recurrence, OccurrenceAction::Exclude)
                .execute(&mut state)
                .unwrap();
            let disk = reload(tmp.path());
            let file = disk.file_by_id("meeting").unwrap();
            assert_eq!(file.components().len(), 1);
            assert!(
                fs::read_to_string(tmp.path().join("event.ics"))
                    .unwrap()
                    .contains(expected)
            );
            let rid: CalDate = recurrence.parse().unwrap();
            let rid = rid.normalize_to(
                file.components()[0].start().unwrap(),
                file.calendar().timezone_resolver(),
            );
            assert!(
                file.occurrence_by_id("meeting", Some(&rid), state.timezone())
                    .unwrap()
                    .is_excluded()
            );
            let other: CalDate = "TU2026-10-14T09:00:00".parse().unwrap();
            assert!(
                !file
                    .occurrence_by_id("meeting", Some(&other), state.timezone())
                    .unwrap()
                    .is_excluded()
            );
            command(recurrence, OccurrenceAction::Exclude)
                .execute(&mut state)
                .unwrap();
            assert!(
                !fs::read_to_string(tmp.path().join("event.ics"))
                    .unwrap()
                    .contains("EXDATE")
            );
        }
    }

    #[test]
    fn cancellation_creates_or_updates_an_override_and_preserves_the_series() {
        for existing_override in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = fixture(
                tmp.path(),
                "DTSTART:20261012T090000Z",
                "ORGANIZER:mailto:host@example.org\r\n",
                false,
            );
            let recurrence = "TU2026-10-13T09:00:00";
            if existing_override {
                let path = tmp.path().join("event.ics");
                let text = fs::read_to_string(&path).unwrap().replace(
                    "END:VCALENDAR",
                    "BEGIN:VEVENT\r\nUID:meeting\r\nRECURRENCE-ID:20261013T090000Z\r\n\
                    DTSTART:20261013T110000Z\r\nSUMMARY:Moved meeting\r\n\
                    END:VEVENT\r\nEND:VCALENDAR",
                );
                fs::write(&path, text).unwrap();
                state.store_mut().retain(|_| false);
                state.store_mut().add(reload(tmp.path()));
            }
            command(recurrence, OccurrenceAction::Cancel)
                .execute(&mut state)
                .unwrap();
            let disk = reload(tmp.path());
            let file = disk.file_by_id("meeting").unwrap();
            assert_eq!(file.components().len(), 2);
            let base = file.component_with(|c| c.rid().is_none()).unwrap();
            assert_eq!(base.summary().map(String::as_str), Some("Meeting"));
            assert_ne!(
                base.as_event().unwrap().status(),
                Some(CalEventStatus::Cancelled)
            );
            let rid: CalDate = recurrence.parse().unwrap();
            let occurrence = file
                .occurrence_by_id("meeting", Some(&rid), state.timezone())
                .unwrap();
            assert!(occurrence.is_cancelled());
            assert!(!occurrence.is_excluded());
            assert_eq!(
                occurrence.summary().unwrap(),
                if existing_override {
                    "Canceled: Moved meeting"
                } else {
                    "Canceled: Meeting"
                }
            );
            let before = fs::read(tmp.path().join("event.ics")).unwrap();
            assert!(
                command(recurrence, OccurrenceAction::Cancel)
                    .execute(&mut state)
                    .is_err()
            );
            assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), before);
        }
    }

    #[test]
    fn failed_actions_preserve_memory_and_disk() {
        for action in [OccurrenceAction::Exclude, OccurrenceAction::Cancel] {
            for case in [
                "read_only",
                "not_owner",
                "missing_item",
                "missing_recurrence",
                "non_recurring",
                "save_failure",
            ] {
                let tmp = tempfile::tempdir().unwrap();
                let organizer = if case == "not_owner" {
                    "ORGANIZER:mailto:other@example.org\r\n"
                } else {
                    ""
                };
                let mut state = fixture(
                    tmp.path(),
                    "DTSTART:20261012T090000Z",
                    organizer,
                    case == "read_only",
                );
                let path = tmp.path().join("event.ics");
                if case == "non_recurring" {
                    let text = fs::read_to_string(&path)
                        .unwrap()
                        .replace("RRULE:FREQ=DAILY;COUNT=5\r\n", "");
                    fs::write(&path, text).unwrap();
                    state.store_mut().retain(|_| false);
                    state.store_mut().add(reload(tmp.path()));
                }
                let before = fs::read(&path).unwrap();
                let mut memory_before = Vec::new();
                state
                    .store()
                    .file_by_id("meeting")
                    .unwrap()
                    .calendar()
                    .write(&mut memory_before)
                    .unwrap();
                let mut cmd = command("TU2026-10-13T09:00:00", action);
                if case == "missing_item" {
                    cmd.item.uid = "missing".into();
                }
                if case == "missing_recurrence" {
                    cmd.item.recurrence = None;
                }
                if case == "save_failure" {
                    fs::remove_file(&path).unwrap();
                    fs::create_dir(&path).unwrap();
                }
                assert!(cmd.execute(&mut state).is_err(), "{action:?}: {case}");
                let mut memory_after = Vec::new();
                state
                    .store()
                    .file_by_id("meeting")
                    .unwrap()
                    .calendar()
                    .write(&mut memory_after)
                    .unwrap();
                assert_eq!(memory_after, memory_before, "{action:?}: {case}");
                if case == "save_failure" {
                    assert!(path.is_dir());
                } else {
                    assert_eq!(fs::read(&path).unwrap(), before, "{action:?}: {case}");
                }
            }
        }
    }
}
