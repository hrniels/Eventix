// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fs, io::Write, sync::Arc};

use anyhow::Context;
use eventix_ical::{
    col::{CalFile, ColError},
    objects::{CalAttendee, CalComponent, CalPartStat, EventLike, UpdatableEventLike},
};

use super::ItemId;
use crate::{State, actions::Action};

/// Sets the current user's attendee response for a series or a single occurrence.
#[derive(Debug)]
pub struct RespondItem {
    pub item: ItemId,
    pub response: CalPartStat,
}

impl Action for RespondItem {
    fn affected_items(&self) -> &[ItemId] {
        std::slice::from_ref(&self.item)
    }

    fn execute(self: Box<Self>, state: &mut State) -> anyhow::Result<ItemId> {
        let locale = state.locale();
        let timezone = *state.timezone();
        let (collection, _) = state.settings().calendar(&self.item.calendar).context(
            locale
                .translate("Calendar is no longer available")
                .to_string(),
        )?;
        let user = collection
            .email()
            .cloned()
            .context(locale.translate("Email account not specified").to_string())?;
        let directory = Arc::new(self.item.calendar.clone());
        let file = state
            .store_mut()
            .directory_mut(&directory)?
            .file_by_id_mut(&self.item.uid)?;
        file.occurrence_by_id(&self.item.uid, self.item.recurrence.as_ref(), &timezone)
            .context(
                locale
                    .translate("Occurrence is no longer available")
                    .to_string(),
            )?;

        // Stage the response so failed persistence leaves the live calendar unchanged.
        let mut staged = CalFile::new(directory, file.path().clone(), file.calendar().clone());
        let modify = |base: Option<&CalComponent>, component: &mut CalComponent| {
            let mut attendees = component
                .attendees()
                .or_else(|| base.and_then(EventLike::attendees))
                .unwrap_or(&[])
                .to_vec();
            if let Some(attendee) = attendees.iter_mut().find(|a| a.address() == user.address()) {
                attendee.set_part_stat(Some(self.response));
            } else {
                let mut attendee = CalAttendee::new(user.address());
                attendee.set_common_name(user.name().clone());
                attendee.set_part_stat(Some(self.response));
                attendees.push(attendee);
            }
            component.set_attendees(Some(attendees));
            component.touch();
            Ok(())
        };
        if let Some(recurrence) = &self.item.recurrence {
            staged.change_single(
                &self.item.uid,
                Some(recurrence),
                &timezone,
                Some(&user.address()),
                false,
                modify,
            )?;
        } else {
            let component = staged
                .component_with_mut(|c| c.uid() == &self.item.uid && c.rid().is_none())
                .ok_or_else(|| ColError::UidNotFound(self.item.uid.clone()))?;
            modify(None, component).map_err(ColError::ModifyFailed)?;
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
        objects::{CalDate, CalPartStat, EventLike},
    };

    use super::{Action, ItemId, RespondItem};
    use crate::{CalendarSettings, CollectionSettings, EmailAccount, State, SyncerType};

    fn fixture(path: &Path, recurrent: bool, read_only: bool) -> State {
        let recurrence = if recurrent {
            "RRULE:FREQ=DAILY;COUNT=5\r\n"
        } else {
            ""
        };
        fs::write(
            path.join("event.ics"),
            format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
                UID:invitation\r\nDTSTART:20261012T090000Z\r\nDTEND:20261012T100000Z\r\n\
                {recurrence}SUMMARY:Meeting\r\nORGANIZER:mailto:host@example.org\r\n\
                ATTENDEE;CN=Guest;ROLE=REQ-PARTICIPANT:mailto:guest@example.org\r\n\
                ATTENDEE;PARTSTAT=ACCEPTED:mailto:other@example.org\r\n\
                END:VEVENT\r\nEND:VCALENDAR\r\n"
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
            email: EmailAccount::new("Guest".into(), "guest@example.org".into()),
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

    fn command(recurrence: Option<CalDate>, response: CalPartStat) -> Box<RespondItem> {
        Box::new(RespondItem {
            item: ItemId {
                calendar: "test".into(),
                uid: "invitation".into(),
                recurrence,
            },
            response,
        })
    }

    #[test]
    fn responses_persist_and_preserve_other_attendees_and_event_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), false, false);
        for response in [
            CalPartStat::Accepted,
            CalPartStat::Tentative,
            CalPartStat::Declined,
        ] {
            command(None, response).execute(&mut state).unwrap();
            let disk = CalDir::new_from_dir(
                Arc::new("test".into()),
                tmp.path().into(),
                "Test".into(),
                false,
            )
            .unwrap();
            let component = &disk.file_by_id("invitation").unwrap().components()[0];
            assert_eq!(component.summary().map(String::as_str), Some("Meeting"));
            assert_eq!(component.organizer().unwrap().address(), "host@example.org");
            let attendees = component.attendees().unwrap();
            assert_eq!(attendees.len(), 2);
            assert_eq!(attendees[0].part_stat(), Some(response));
            assert_eq!(
                attendees[0].common_name().map(String::as_str),
                Some("Guest")
            );
            assert_eq!(attendees[1].part_stat(), Some(CalPartStat::Accepted));
        }
    }

    #[test]
    fn occurrence_responses_inherit_attendees_and_series_responses_preserve_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), true, false);
        let recurrence: CalDate = "TU2026-10-13T09:00:00".parse().unwrap();
        for response in [CalPartStat::Tentative, CalPartStat::Declined] {
            command(Some(recurrence.clone()), response)
                .execute(&mut state)
                .unwrap();
            let file = state.store().file_by_id("invitation").unwrap();
            assert_eq!(file.components().len(), 2);
            let base = file
                .components()
                .iter()
                .find(|c| c.rid().is_none())
                .unwrap();
            assert_eq!(base.attendees().unwrap()[0].part_stat(), None);
            let occurrence = file
                .occurrence_by_id("invitation", Some(&recurrence), state.timezone())
                .unwrap();
            assert_eq!(
                occurrence.attendees().unwrap()[0].part_stat(),
                Some(response)
            );
            assert_eq!(
                occurrence.attendees().unwrap()[1].part_stat(),
                Some(CalPartStat::Accepted)
            );
        }
        command(None, CalPartStat::Accepted)
            .execute(&mut state)
            .unwrap();
        let file = state.store().file_by_id("invitation").unwrap();
        for component in file.components() {
            assert_eq!(
                component.attendees().unwrap()[0].part_stat(),
                Some(if component.rid().is_some() {
                    CalPartStat::Declined
                } else {
                    CalPartStat::Accepted
                })
            );
        }
    }

    #[test]
    fn missing_attendee_is_added_using_the_collection_account() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), false, false);
        let collection = state
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap();
        collection.set_syncer(SyncerType::VDirSyncer {
            email: EmailAccount::new("New guest".into(), "new@example.org".into()),
            url: "https://example.org".into(),
            read_only: false,
            username: None,
            password: None,
            time_span: Default::default(),
        });
        command(None, CalPartStat::Accepted)
            .execute(&mut state)
            .unwrap();
        let file = state.store().file_by_id("invitation").unwrap();
        let attendees = file.components()[0].attendees().unwrap();
        assert_eq!(attendees.len(), 3);
        assert_eq!(attendees[2].address(), "new@example.org");
        assert_eq!(
            attendees[2].common_name().map(String::as_str),
            Some("New guest")
        );
        assert_eq!(attendees[2].part_stat(), Some(CalPartStat::Accepted));
    }

    #[test]
    fn failed_responses_preserve_memory_and_disk() {
        for case in ["read_only", "missing_email", "missing_item", "save_failure"] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = fixture(tmp.path(), false, case == "read_only");
            let path = tmp.path().join("event.ics");
            let before = fs::read(&path).unwrap();
            if case == "missing_email" {
                state
                    .settings_mut()
                    .collections_mut()
                    .get_mut("local")
                    .unwrap()
                    .set_syncer(SyncerType::FileSystem {
                        path: tmp.path().display().to_string(),
                    });
            }
            let mut response = command(None, CalPartStat::Accepted);
            if case == "missing_item" {
                response.item.uid = "missing".into();
            }
            if case == "save_failure" {
                fs::remove_file(&path).unwrap();
                fs::create_dir(&path).unwrap();
            }
            assert!(response.execute(&mut state).is_err(), "{case}");
            let file = state.store().file_by_id("invitation").unwrap();
            assert_eq!(
                file.components()[0].attendees().unwrap()[0].part_stat(),
                None
            );
            if case == "save_failure" {
                assert!(path.is_dir());
            } else {
                assert_eq!(fs::read(&path).unwrap(), before, "{case}");
            }
        }
    }
}
