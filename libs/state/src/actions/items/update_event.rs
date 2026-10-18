// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fs, io::Write, path::Path, sync::Arc, time::SystemTime};

use anyhow::{Context, bail};
use eventix_ical::{
    col::{CalFile, Occurrence},
    objects::{
        CalComponent, CalDate, CalDateTime, CalOrganizer, CalRRule, EventLike, UpdatableEventLike,
    },
};
use eventix_locale::Locale;

use super::{CreateEvent, ItemId};
use crate::State;

/// Selects which part of an event series is edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditScope {
    Occurrence,
    Following,
    Series,
}

/// An owned edit draft with the file revision against which it was prepared.
#[derive(Clone, Debug)]
pub struct UpdateEvent {
    pub item: ItemId,
    pub scope: EditScope,
    pub fields: CreateEvent,
    revision: SystemTime,
}

impl UpdateEvent {
    /// Loads an editable event, rejecting unavailable calendars and events without edit permission.
    pub fn load(state: &State, mut item: ItemId, scope: EditScope) -> anyhow::Result<Self> {
        let locale = state.locale();
        let (collection, _) = state.settings().calendar(&item.calendar).context(
            locale
                .translate("Calendar is no longer available")
                .to_string(),
        )?;

        if scope == EditScope::Series {
            item.recurrence = None;
        }

        let file = state.store().file_by_id(&item.uid).context(
            locale
                .translate("Occurrence is no longer available")
                .to_string(),
        )?;
        let occ = file
            .occurrence_by_id(&item.uid, item.recurrence.as_ref(), state.timezone())
            .context(
                locale
                    .translate("Occurrence is no longer available")
                    .to_string(),
            )?;

        let mail = collection.email().map(|email| email.address());
        if collection.is_read_only()
            || file.directory().as_str() != item.calendar
            || !occ.is_owned_by(mail.as_ref())
            || occ.ctype() != eventix_ical::objects::CalCompType::Event
        {
            bail!(locale.translate("Could not apply changes").to_string());
        }

        let mut fields = Self::load_fields(state, &item, scope, &occ)?;
        if scope == EditScope::Following {
            Self::adjust_following_count(state, file, &item, &mut fields)?;
        }

        Ok(Self {
            item,
            scope,
            fields,
            revision: file.last_modified()?,
        })
    }

    fn load_fields(
        state: &State,
        item: &ItemId,
        scope: EditScope,
        occ: &Occurrence<'_>,
    ) -> anyhow::Result<CreateEvent> {
        let locale = state.locale();
        let personal_alarms = state
            .personal_alarms()
            .get(&item.calendar)
            .and_then(|alarms| alarms.get(&item.uid, item.recurrence.as_ref()))
            .map(|alarms| alarms.alarms().to_vec());
        Ok(CreateEvent {
            calendar: item.calendar.clone(),
            summary: occ.summary().cloned().unwrap_or_default(),
            location: occ.location().cloned().unwrap_or_default(),
            description: occ.description().cloned().unwrap_or_default(),
            start: occ
                .occurrence_startdate()
                .context(locale.translate("error.valid_date_time").to_string())?,
            end: occ
                .occurrence_enddate()
                .context(locale.translate("error.valid_date_time").to_string())?,
            recurrence: if scope == EditScope::Occurrence {
                None
            } else {
                occ.rrule().cloned()
            },
            alarms: occ.alarms().unwrap_or_default().to_vec(),
            attendees: occ.attendees().unwrap_or_default().to_vec(),
            personal_alarms,
        })
    }

    fn adjust_following_count(
        state: &State,
        file: &CalFile,
        item: &ItemId,
        fields: &mut CreateEvent,
    ) -> anyhow::Result<()> {
        let locale = state.locale();
        if let Some(rule) = fields.recurrence.as_mut()
            && let Some(count) = rule.count()
        {
            let base = file
                .component_with(|c| c.uid() == &item.uid && c.rid().is_none())
                .context(
                    locale
                        .translate("Occurrence is no longer available")
                        .to_string(),
                )?;
            let ctx = file.calendar().date_context();

            let first = ctx
                .date(
                    base.start()
                        .context(locale.translate("error.valid_date_time").to_string())?,
                )
                .start_in(state.timezone());

            let selected = ctx
                .date(
                    item.recurrence
                        .as_ref()
                        .context(locale.translate("Invalid recurrence").to_string())?,
                )
                .start_in(state.timezone());

            let preceding = file
                .occurrences_between(first, selected, |c| c.uid() == &item.uid)
                .count() as u64;
            rule.set_count(count.saturating_sub(preceding).max(1));
        }
        Ok(())
    }

    /// Persists edited fields, rejecting stale revisions and retaining properties outside the form.
    pub fn execute(self, state: &mut State) -> anyhow::Result<ItemId> {
        let current = self.validate_revision(state)?;
        let locale = state.locale();
        let tz = *state.timezone();
        let collection = state.settings().calendar(&self.item.calendar).unwrap().0;
        let organizer = collection.build_organizer();
        let mail = collection.email().map(|email| email.address().clone());

        let file = state
            .store_mut()
            .directory_mut(&Arc::new(self.item.calendar.clone()))?
            .file_by_id_mut(&self.item.uid)?;
        let mut staged = CalFile::new(
            file.directory().clone(),
            file.path().clone(),
            file.calendar().clone(),
        );

        let mut result = self.item.clone();
        let mut following = None;
        let apply = |component: &mut CalComponent| {
            self.apply_fields(component, &current.fields, organizer.as_ref());
        };

        match self.scope {
            EditScope::Occurrence if self.item.recurrence.is_some() => {
                staged.change_single(
                    &self.item.uid,
                    self.item.recurrence.as_ref(),
                    &tz,
                    mail.as_ref(),
                    true,
                    |_, component| {
                        apply(component);
                        Ok(())
                    },
                )?;
            }
            EditScope::Following if self.item.recurrence.is_some() => {
                following =
                    self.split_following(&mut staged, &mut result, apply, locale.as_ref())?;
            }
            _ => {
                self.update_series(&mut staged, &current.fields, apply, locale.as_ref())?;
                result.recurrence = None;
            }
        }

        staged.calendar_mut().populate_timezones();
        persist(&staged, following.as_ref(), locale.as_ref())?;
        *file.calendar_mut() = staged.calendar().clone();
        if let Some(next) = following {
            state
                .store_mut()
                .directory_mut(&Arc::new(result.calendar.clone()))?
                .add_file(next)?;
        }

        self.save_personal_alarms(state, &result);
        Ok(result)
    }

    fn validate_revision(&self, state: &State) -> anyhow::Result<Self> {
        self.fields.validate(state)?;
        let current = Self::load(state, self.item.clone(), self.scope)?;
        let locale = state.locale();
        if current.revision != self.revision {
            bail!(locale.translate("error.component_was_modified").to_string());
        }
        // Calendar relocation is a separate operation; occurrence edits stay in their calendar.
        if self.fields.calendar != self.item.calendar {
            bail!(locale.translate("Could not apply changes").to_string());
        }
        Ok(current)
    }

    /// Applies edited fields while leaving untouched occurrence properties inherited.
    fn apply_fields(
        &self,
        component: &mut CalComponent,
        current: &CreateEvent,
        organizer: Option<&CalOrganizer>,
    ) {
        let occurrence = self.scope == EditScope::Occurrence;
        if !occurrence || self.fields.summary != current.summary {
            component.set_summary(Some(self.fields.summary.clone()));
        }
        if !occurrence || self.fields.location != current.location {
            component.set_location(Some(self.fields.location.clone()));
        }
        if !occurrence || self.fields.description != current.description {
            component.set_description(Some(self.fields.description.clone()));
        }
        component.set_start(Some(self.fields.start.clone()));
        if let Some(event) = component.as_event_mut() {
            event.set_end(Some(self.fields.end.clone()));
        }
        if !occurrence || self.fields.alarms != current.alarms {
            component.set_alarms(Some(self.fields.alarms.clone()));
        }
        if !occurrence || self.fields.attendees != current.attendees {
            component.set_attendees(Some(self.fields.attendees.clone()));
        }
        if !self.fields.attendees.is_empty() && component.organizer().is_none() {
            component.set_organizer(organizer.cloned());
        }
        component.touch();
    }

    fn update_series(
        &self,
        staged: &mut CalFile,
        current: &CreateEvent,
        apply: impl Fn(&mut CalComponent),
        locale: &dyn Locale,
    ) -> anyhow::Result<()> {
        if self.fields.start != current.start || self.fields.end != current.end {
            staged.change_series_range(
                &self.item.uid,
                self.fields.start.clone(),
                Some(self.fields.end.clone()),
            )?;
        }
        let component = staged
            .component_with_mut(|c| c.uid() == &self.item.uid && c.rid().is_none())
            .context(
                locale
                    .translate("Occurrence is no longer available")
                    .to_string(),
            )?;
        apply(component);
        component.set_rrule(self.fields.recurrence.clone());
        Ok(())
    }

    fn save_personal_alarms(self, state: &mut State, item: &ItemId) {
        let personal = state.personal_alarms_mut().get_or_create(&item.calendar);
        if let Some(alarms) = self.fields.personal_alarms {
            personal.set(&item.uid, item.recurrence.as_ref(), alarms);
        } else {
            personal.unset(&item.uid, item.recurrence.as_ref());
        }
        if let Err(error) = personal.save() {
            tracing::warn!(%error, "Unable to save personal alarms");
        }
    }

    /// Splits before the selected recurrence, dropping superseded future exceptions.
    fn split_following(
        &self,
        staged: &mut CalFile,
        result: &mut ItemId,
        apply: impl Fn(&mut CalComponent),
        locale: &dyn Locale,
    ) -> anyhow::Result<Option<CalFile>> {
        let rid = self
            .item
            .recurrence
            .as_ref()
            .context(locale.translate("Invalid recurrence").to_string())?;
        let base = staged
            .component_with_mut(|c| c.uid() == &self.item.uid && c.rid().is_none())
            .context(
                locale
                    .translate("Occurrence is no longer available")
                    .to_string(),
            )?;
        let mut next = base.clone();
        let first = base.start() == Some(rid);
        let rule = base
            .rrule()
            .context(locale.translate("Invalid recurrence").to_string())?;
        base.set_rrule(Some(recurrence_before(rule, rid, locale)?));
        base.touch();
        staged.calendar_mut().delete_components(|c| {
            c.uid() == &self.item.uid && (first || c.rid().is_some_and(|date| date >= rid))
        });
        result.uid = uuid::Uuid::new_v4().to_string();
        result.recurrence = None;
        next.set_uid(result.uid.clone());
        next.set_rrule(self.fields.recurrence.clone());
        apply(&mut next);
        if first {
            staged.calendar_mut().add_component(next);
            return Ok(None);
        }
        let mut calendar = staged.calendar().clone();
        calendar.delete_components(|_| true);
        calendar.add_component(next);
        calendar.populate_timezones();
        Ok(Some(CalFile::new(
            staged.directory().clone(),
            staged
                .path()
                .parent()
                .context(
                    locale
                        .translate("Calendar file has no parent directory")
                        .to_string(),
                )?
                .join(format!("{}.ics", result.uid)),
            calendar,
        )))
    }
}

/// Ends a recurrence immediately before the selected date, retaining its original pattern.
fn recurrence_before(
    rule: &CalRRule,
    rid: &CalDate,
    locale: &dyn Locale,
) -> anyhow::Result<CalRRule> {
    // COUNT and UNTIL are mutually exclusive.
    let mut rule: CalRRule = rule
        .to_string()
        .split(';')
        .filter(|part| !part.starts_with("COUNT=") && !part.starts_with("UNTIL="))
        .collect::<Vec<_>>()
        .join(";")
        .parse()?;
    let until = match rid {
        CalDate::Date(date, kind) => CalDate::Date(
            date.pred_opt()
                .context(locale.translate("error.valid_date_time").to_string())?,
            *kind,
        ),
        _ => match rid.to_utc() {
            CalDate::DateTime(CalDateTime::Utc(date)) => {
                CalDate::DateTime(CalDateTime::Utc(date - chrono::Duration::seconds(1)))
            }
            _ => unreachable!(),
        },
    };
    rule.set_until(until);
    Ok(rule)
}

/// Stages both files before committing a split, rolling back the new file on replacement failure.
fn persist(
    staged: &CalFile,
    following: Option<&CalFile>,
    locale: &dyn Locale,
) -> anyhow::Result<()> {
    let path = staged.path();
    let output = stage_file(staged, path, locale)?;
    if let Some(next) = following {
        let new_output = stage_file(next, path, locale)?;
        new_output.persist_noclobber(next.path())?;
    }
    if let Err(error) = output.persist(path) {
        if let Some(next) = following {
            let _ = fs::remove_file(next.path());
        }
        return Err(error.into());
    }
    Ok(())
}

/// Writes and syncs a temporary calendar file using the original file's directory and permissions.
fn stage_file(
    file: &CalFile,
    original_path: &Path,
    locale: &dyn Locale,
) -> anyhow::Result<tempfile::NamedTempFile> {
    let parent = original_path.parent().context(
        locale
            .translate("Calendar file has no parent directory")
            .to_string(),
    )?;
    let mut output = tempfile::NamedTempFile::new_in(parent)?;
    output
        .as_file()
        .set_permissions(fs::metadata(original_path)?.permissions())?;
    file.calendar().write(output.as_file_mut())?;
    output.flush()?;
    output.as_file().sync_all()?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use chrono::TimeZone;
    use eventix_ical::{
        col::CalDir,
        objects::{CalDate, CalPartStat, EventLike},
    };

    use super::{EditScope, ItemId, UpdateEvent};
    use crate::{CalendarSettings, CollectionSettings, State, SyncerType};

    fn fixture(path: &Path, recurrent: bool) -> State {
        let recurrence = if recurrent {
            "RRULE:FREQ=DAILY;COUNT=5\r\n"
        } else {
            ""
        };
        fs::write(
            path.join("event.ics"),
            format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:edit-me\r\n\
             DTSTART:20261012T090000Z\r\nDTEND:20261012T100000Z\r\n{recurrence}\
             SUMMARY:Original\r\nSTATUS:CONFIRMED\r\nX-CUSTOM:keep-me\r\n\
             ATTENDEE;CN=Guest;PARTSTAT=ACCEPTED:mailto:guest@example.org\r\n\
             END:VEVENT\r\nEND:VCALENDAR\r\n"
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
            path: path.display().to_string(),
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

    fn item(recurrence: Option<&str>) -> ItemId {
        ItemId {
            calendar: "test".into(),
            uid: "edit-me".into(),
            recurrence: recurrence.map(|rid| rid.parse().unwrap()),
        }
    }

    #[test]
    fn saves_existing_event_preserving_identity_metadata_and_attendee_responses() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), false);
        let mut edit = UpdateEvent::load(&state, item(None), EditScope::Series).unwrap();
        let stale = edit.clone();
        edit.fields.summary = "Edited".into();
        edit.fields.location = "Office".into();
        assert_eq!(edit.execute(&mut state).unwrap(), item(None));
        let disk = fs::read_to_string(tmp.path().join("event.ics")).unwrap();
        assert!(disk.contains("SUMMARY:Edited"));
        assert!(disk.contains("X-CUSTOM:keep-me"));
        assert!(disk.contains("STATUS:CONFIRMED"));
        let occurrence = state
            .store()
            .occurrence_by_id("edit-me", None, state.timezone())
            .unwrap();
        assert_eq!(
            occurrence.attendees().unwrap()[0].part_stat(),
            Some(CalPartStat::Accepted)
        );
        assert!(stale.execute(&mut state).is_err());
        assert_eq!(
            fs::read_to_string(tmp.path().join("event.ics")).unwrap(),
            disk
        );
    }

    #[test]
    fn occurrence_edit_changes_only_selected_occurrence_and_keeps_its_original_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), true);
        let target = item(Some("TU2026-10-13T09:00:00"));
        let mut edit = UpdateEvent::load(&state, target.clone(), EditScope::Occurrence).unwrap();
        edit.fields.summary = "Exception".into();
        edit.fields.start = "TU2026-10-13T12:00:00".parse().unwrap();
        edit.fields.end = "TU2026-10-13T13:00:00".parse().unwrap();
        assert_eq!(edit.execute(&mut state).unwrap(), target);
        let occurrence = state
            .store()
            .occurrence_by_id("edit-me", target.recurrence.as_ref(), state.timezone())
            .unwrap();
        assert_eq!(occurrence.summary().unwrap(), "Exception");
        assert_eq!(
            occurrence.occurrence_startdate().unwrap().to_utc(),
            "TU2026-10-13T12:00:00".parse::<CalDate>().unwrap()
        );
        let other = state
            .store()
            .occurrence_by_id(
                "edit-me",
                Some(&"TU2026-10-14T09:00:00".parse().unwrap()),
                state.timezone(),
            )
            .unwrap();
        assert_eq!(other.summary().unwrap(), "Original");
        assert_eq!(other.rrule().unwrap().count(), Some(5));
        let mut series = UpdateEvent::load(&state, item(None), EditScope::Series).unwrap();
        series.fields.description = "Inherited description".into();
        series.execute(&mut state).unwrap();
        let occurrence = state
            .store()
            .occurrence_by_id("edit-me", target.recurrence.as_ref(), state.timezone())
            .unwrap();
        assert_eq!(occurrence.summary().unwrap(), "Exception");
        assert_eq!(occurrence.description().unwrap(), "Inherited description");
    }

    #[test]
    fn following_edit_splits_series_and_preserves_remaining_count() {
        for day in [12, 14] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = fixture(tmp.path(), true);
            let target = item(Some(&format!("TU2026-10-{day}T09:00:00")));
            let mut edit = UpdateEvent::load(&state, target, EditScope::Following).unwrap();
            assert_eq!(
                edit.fields.recurrence.as_ref().unwrap().count(),
                Some(5 - (day - 12))
            );
            edit.fields.summary = "Following".into();
            let result = edit.execute(&mut state).unwrap();
            assert_ne!(result.uid, "edit-me");
            let start = chrono_tz::UTC
                .with_ymd_and_hms(2026, 10, 12, 0, 0, 0)
                .unwrap();
            let end = start + chrono::Duration::days(10);
            let occurrences: Vec<_> = state.store().directories()[0]
                .occurrences_between(start, end, |_| true)
                .collect();
            assert_eq!(occurrences.len(), 5);
            for occ in occurrences {
                let date = occ.occurrence_start().unwrap().date_naive();
                assert_eq!(
                    occ.summary().unwrap(),
                    if date < format!("2026-10-{day}").parse().unwrap() {
                        "Original"
                    } else {
                        "Following"
                    }
                );
            }
            let reloaded = CalDir::new_from_dir(
                Arc::new("test".into()),
                tmp.path().into(),
                "Test".into(),
                false,
            )
            .unwrap();
            assert_eq!(
                reloaded.occurrences_between(start, end, |_| true).count(),
                5
            );
        }
    }

    #[test]
    fn invalid_edits_and_changed_files_do_not_modify_live_state_or_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), false);
        let mut edit = UpdateEvent::load(&state, item(None), EditScope::Series).unwrap();
        let original = fs::read(tmp.path().join("event.ics")).unwrap();
        edit.fields.summary.clear();
        assert!(edit.execute(&mut state).is_err());
        assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), original);
        let edit = UpdateEvent::load(&state, item(None), EditScope::Series).unwrap();
        fs::write(tmp.path().join("event.ics"), b"external change").unwrap();
        assert!(edit.execute(&mut state).is_err());
        assert_eq!(
            fs::read(tmp.path().join("event.ics")).unwrap(),
            b"external change"
        );
    }
}
