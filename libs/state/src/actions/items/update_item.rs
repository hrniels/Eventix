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

use super::{CreateItem, ItemId, TaskStatus};
use crate::{State, actions::Action};

/// Selects which part of an event or task series is edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditScope {
    Occurrence,
    Following,
    Series,
}

/// An owned edit draft with the file revision against which it was prepared.
#[derive(Clone, Debug)]
pub struct UpdateItem {
    pub item: ItemId,
    pub scope: EditScope,
    pub fields: CreateItem,
    revision: SystemTime,
}

impl UpdateItem {
    /// Loads an editable item, rejecting unavailable calendars and items without edit permission.
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
    ) -> anyhow::Result<CreateItem> {
        let personal_alarms = state
            .personal_alarms()
            .get(&item.calendar)
            .and_then(|alarms| alarms.get(&item.uid, item.recurrence.as_ref()))
            .map(|alarms| alarms.alarms().to_vec());
        Ok(CreateItem {
            kind: occ.ctype(),
            task_status: (occ.ctype() == eventix_ical::objects::CalCompType::Todo).then(|| {
                TaskStatus {
                    status: occ
                        .todo_status()
                        .unwrap_or(eventix_ical::objects::CalTodoStatus::NeedsAction),
                    percent: occ.todo_percent(),
                    completed: occ.todo_completed().cloned(),
                }
            }),
            calendar: item.calendar.clone(),
            summary: occ.summary().cloned().unwrap_or_default(),
            location: occ.location().cloned().unwrap_or_default(),
            description: occ.description().cloned().unwrap_or_default(),
            start: occ.occurrence_startdate(),
            end: occ.occurrence_enddate(),
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
        fields: &mut CreateItem,
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
}

impl Action for UpdateItem {
    fn affected_items(&self) -> &[ItemId] {
        std::slice::from_ref(&self.item)
    }

    fn execute(self: Box<Self>, state: &mut State) -> anyhow::Result<ItemId> {
        let current = self.validate_revision(state)?;
        let locale = state.locale();
        let tz = *state.timezone();
        let collection = state.settings().calendar(&self.item.calendar).unwrap().0;
        let mail = collection.email().map(|email| email.address().clone());
        let organizer = state
            .settings()
            .calendar(&self.fields.calendar)
            .unwrap()
            .0
            .build_organizer();
        let target_path = state
            .store_mut()
            .directory_mut(&Arc::new(self.fields.calendar.clone()))?
            .path()
            .clone();

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
                following = self.split_following(
                    &mut staged,
                    &mut result,
                    &target_path,
                    apply,
                    locale.as_ref(),
                )?;
            }
            _ => {
                self.update_series(&mut staged, &current.fields, apply, locale.as_ref())?;
                result.recurrence = None;
            }
        }

        staged.calendar_mut().populate_timezones();
        result.calendar = self.fields.calendar.clone();
        if self.fields.calendar != self.item.calendar && following.is_none() {
            let original = file.calendar().clone();
            let path = file.path().clone();
            let name = path
                .file_name()
                .context(locale.translate("Could not apply changes").to_string())?;
            if target_path.join(name).exists() {
                bail!(locale.translate("Could not apply changes").to_string());
            }
            *file.calendar_mut() = staged.calendar().clone();
            let source = Arc::new(self.item.calendar.clone());
            let target = Arc::new(self.fields.calendar.clone());
            if let Err(error) = state.store_mut().switch_directory(path, &source, &target) {
                let file = state
                    .store_mut()
                    .directory_mut(&source)?
                    .file_by_id_mut(&result.uid)?;
                *file.calendar_mut() = original;
                persist(file, None, locale.as_ref())?;
                return Err(error.into());
            }
        } else {
            persist(&staged, following.as_ref(), locale.as_ref())?;
            *file.calendar_mut() = staged.calendar().clone();
        }
        if let Some(next) = following {
            state
                .store_mut()
                .directory_mut(&Arc::new(result.calendar.clone()))?
                .add_file(next)?;
        }

        self.save_personal_alarms(state, &result);
        Ok(result)
    }
}

impl UpdateItem {
    fn validate_revision(&self, state: &State) -> anyhow::Result<Self> {
        self.fields.validate(state)?;
        let current = Self::load(state, self.item.clone(), self.scope)?;
        let locale = state.locale();
        if current.fields.kind != self.fields.kind {
            bail!(locale.translate("Could not apply changes").to_string());
        }
        if current.revision != self.revision {
            bail!(locale.translate("error.component_was_modified").to_string());
        }
        // A single occurrence must remain in the calendar of its parent series.
        if self.fields.calendar != self.item.calendar && self.scope == EditScope::Occurrence {
            bail!(locale.translate("Could not apply changes").to_string());
        }
        if self.fields.calendar != self.item.calendar {
            let target = state.settings().calendar(&self.fields.calendar);
            if !target.is_some_and(|(collection, calendar)| {
                !collection.is_read_only() && calendar.types().contains(&self.fields.kind)
            }) {
                bail!(locale.translate("Could not apply changes").to_string());
            }
        }
        Ok(current)
    }

    /// Applies edited fields while leaving untouched occurrence properties inherited.
    fn apply_fields(
        &self,
        component: &mut CalComponent,
        current: &CreateItem,
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
        component.set_start(self.fields.start.clone());
        if let Some(event) = component.as_event_mut() {
            event.set_end(self.fields.end.clone());
        } else if let Some(task) = component.as_todo_mut() {
            task.set_due(self.fields.end.clone());
            if self.fields.task_status != current.task_status
                && let Some(status) = &self.fields.task_status
            {
                status.apply(task);
                if task.priority().is_none() {
                    task.set_priority(Some(eventix_ical::objects::PRIORITY_MEDIUM));
                }
            }
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
        current: &CreateItem,
        apply: impl Fn(&mut CalComponent),
        locale: &dyn Locale,
    ) -> anyhow::Result<()> {
        if (self.fields.start != current.start || self.fields.end != current.end)
            && let Some(start) = &self.fields.start
        {
            staged.change_series_range(&self.item.uid, start.clone(), self.fields.end.clone())?;
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
        target_path: &Path,
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
            Arc::new(self.fields.calendar.clone()),
            target_path.join(format!("{}.ics", result.uid)),
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

/// Writes and syncs a temporary calendar file beside its destination with the original permissions.
fn stage_file(
    file: &CalFile,
    original_path: &Path,
    locale: &dyn Locale,
) -> anyhow::Result<tempfile::NamedTempFile> {
    let parent = file.path().parent().context(
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
        objects::{CalCompType, CalDate, CalPartStat, EventLike},
    };

    use super::{Action, EditScope, ItemId, UpdateItem};
    use crate::{CalendarSettings, CollectionSettings, State, SyncerType};

    fn fixture(path: &Path, recurrent: bool) -> State {
        fixture_kind(path, recurrent, false)
    }

    fn fixture_kind(path: &Path, recurrent: bool, task: bool) -> State {
        let recurrence = if recurrent {
            "RRULE:FREQ=DAILY;COUNT=5\r\n"
        } else {
            ""
        };
        let content = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:edit-me\r\n\
             DTSTART:20261012T090000Z\r\nDTEND:20261012T100000Z\r\n{recurrence}\
             SUMMARY:Original\r\nSTATUS:CONFIRMED\r\nX-CUSTOM:keep-me\r\n\
             ATTENDEE;CN=Guest;PARTSTAT=ACCEPTED:mailto:guest@example.org\r\n\
             END:VEVENT\r\nEND:VCALENDAR\r\n"
        );
        let content = if task {
            content
                .replace("VEVENT", "VTODO")
                .replace("DTEND", "DUE")
                .replace("STATUS:CONFIRMED", "STATUS:NEEDS-ACTION")
        } else {
            content
        };
        fs::write(path.join("event.ics"), content).unwrap();
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

    fn add_target(state: &mut State, path: &Path, read_only: bool) {
        state.store_mut().add(CalDir::new_empty(
            Arc::new("target".into()),
            path.into(),
            "Target".into(),
            read_only,
        ));
        let mut calendar = CalendarSettings::default();
        calendar.set_enabled(true);
        calendar.set_types(vec![CalCompType::Event]);
        let mut collection = CollectionSettings::new(SyncerType::VDirSyncer {
            email: crate::EmailAccount::new("Owner".into(), "owner@example.org".into()),
            url: "https://example.org".into(),
            read_only,
            username: None,
            password: None,
            time_span: Default::default(),
        });
        collection
            .all_calendars_mut()
            .insert("target".into(), calendar);
        state
            .settings_mut()
            .collections_mut()
            .insert("target".into(), collection);
    }

    #[test]
    fn task_edits_save_status_progress_completion_and_optional_dates() {
        use super::TaskStatus;
        use eventix_ical::objects::CalTodoStatus;

        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture_kind(tmp.path(), false, true);
        let mut edit = UpdateItem::load(&state, item(None), EditScope::Series).unwrap();
        assert_eq!(edit.fields.kind, CalCompType::Todo);
        edit.fields.start = None;
        edit.fields.end = None;
        edit.fields.task_status = Some(TaskStatus {
            status: CalTodoStatus::InProcess,
            percent: Some(35),
            completed: None,
        });
        Box::new(edit).execute(&mut state).unwrap();
        let mut edit = UpdateItem::load(&state, item(None), EditScope::Series).unwrap();
        assert!(edit.fields.start.is_none() && edit.fields.end.is_none());
        assert_eq!(edit.fields.task_status.as_ref().unwrap().percent, Some(35));
        edit.fields.task_status = Some(TaskStatus {
            status: CalTodoStatus::Completed,
            percent: None,
            completed: Some("TU2026-10-15T00:00:00".parse().unwrap()),
        });
        Box::new(edit).execute(&mut state).unwrap();
        let disk = fs::read_to_string(tmp.path().join("event.ics")).unwrap();
        assert!(disk.contains("STATUS:COMPLETED"));
        assert!(disk.contains("PERCENT-COMPLETE:100"));
        assert!(disk.contains("COMPLETED:20261015T000000Z"));
        assert!(disk.contains("X-CUSTOM:keep-me"));
        let mut edit = UpdateItem::load(&state, item(None), EditScope::Series).unwrap();
        edit.fields.task_status.as_mut().unwrap().status = CalTodoStatus::NeedsAction;
        Box::new(edit).execute(&mut state).unwrap();
        let disk = fs::read_to_string(tmp.path().join("event.ics")).unwrap();
        assert!(!disk.contains("PERCENT-COMPLETE"));
        assert!(!disk.contains("COMPLETED:"));
    }

    #[test]
    fn recurring_tasks_support_occurrence_following_and_series_edits() {
        for scope in [
            EditScope::Occurrence,
            EditScope::Following,
            EditScope::Series,
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = fixture_kind(tmp.path(), true, true);
            let target = item(Some("TU2026-10-14T09:00:00"));
            let mut edit = UpdateItem::load(&state, target, scope).unwrap();
            edit.fields.summary = "Changed task".into();
            Box::new(edit).execute(&mut state).unwrap();
            let start = chrono_tz::UTC
                .with_ymd_and_hms(2026, 10, 12, 0, 0, 0)
                .unwrap();
            let mut tasks: Vec<_> = state.store().directories()[0]
                .occurrences_between(start, start + chrono::Duration::days(7), |_| true)
                .collect();
            tasks.sort_by_key(|task| task.occurrence_start());
            assert_eq!(tasks.len(), 5);
            for (index, task) in tasks.iter().enumerate() {
                let changed = match scope {
                    EditScope::Occurrence => index == 2,
                    EditScope::Following => index >= 2,
                    EditScope::Series => true,
                };
                assert_eq!(
                    task.summary().unwrap(),
                    if changed { "Changed task" } else { "Original" }
                );
                assert_eq!(task.ctype(), CalCompType::Todo);
            }
        }
    }

    #[test]
    fn series_edit_moves_standalone_events_and_complete_series_with_exceptions() {
        for recurrent in [false, true] {
            let source = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            let mut state = fixture(source.path(), recurrent);
            add_target(&mut state, target.path(), false);
            let rid = "TU2026-10-13T09:00:00";
            if recurrent {
                let mut exception =
                    UpdateItem::load(&state, item(Some(rid)), EditScope::Occurrence).unwrap();
                exception.fields.summary = "Exception".into();
                Box::new(exception).execute(&mut state).unwrap();
            }
            let mut edit = UpdateItem::load(&state, item(None), EditScope::Series).unwrap();
            edit.fields.calendar = "target".into();
            edit.fields.summary = "Moved".into();
            let result = Box::new(edit).execute(&mut state).unwrap();
            assert_eq!(result.calendar, "target");
            assert_eq!(result.uid, "edit-me");
            assert!(!source.path().join("event.ics").exists());
            let disk = fs::read_to_string(target.path().join("event.ics")).unwrap();
            assert!(disk.contains("SUMMARY:Moved"));
            assert!(disk.contains("X-CUSTOM:keep-me"));
            let file = state.store().file_by_id("edit-me").unwrap();
            assert_eq!(file.directory().as_str(), "target");
            assert_eq!(file.path(), &target.path().join("event.ics"));
            assert!(UpdateItem::load(&state, result, EditScope::Series).is_ok());
            if recurrent {
                let occurrence = state
                    .store()
                    .occurrence_by_id("edit-me", Some(&rid.parse().unwrap()), state.timezone())
                    .unwrap();
                assert_eq!(occurrence.summary().unwrap(), "Exception");
            }
        }
    }

    #[test]
    fn following_edit_moves_only_selected_and_later_occurrences_to_target_calendar() {
        for day in [12, 14] {
            let source = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            let mut state = fixture(source.path(), true);
            add_target(&mut state, target.path(), false);
            let mut edit = UpdateItem::load(
                &state,
                item(Some(&format!("TU2026-10-{day}T09:00:00"))),
                EditScope::Following,
            )
            .unwrap();
            edit.fields.calendar = "target".into();
            edit.fields.summary = "Moved following".into();
            let result = Box::new(edit).execute(&mut state).unwrap();
            assert_eq!(result.calendar, "target");
            assert_ne!(result.uid, "edit-me");
            assert!(result.recurrence.is_none());
            assert!(UpdateItem::load(&state, result, EditScope::Series).is_ok());

            let start = chrono_tz::UTC
                .with_ymd_and_hms(2026, 10, 12, 0, 0, 0)
                .unwrap();
            let end = start + chrono::Duration::days(10);
            for (id, path, expected) in [
                ("test", source.path(), day - 12),
                ("target", target.path(), 17 - day),
            ] {
                let directory = state.store().directory(&Arc::new(id.into())).unwrap();
                let occurrences: Vec<_> = directory
                    .occurrences_between(start, end, |_| true)
                    .collect();
                assert_eq!(occurrences.len(), expected as usize);
                for occurrence in occurrences {
                    let date = occurrence.occurrence_start().unwrap().date_naive();
                    let selected = format!("2026-10-{day}").parse().unwrap();
                    if id == "test" {
                        assert!(date < selected);
                        assert_eq!(occurrence.summary().unwrap(), "Original");
                    } else {
                        assert!(date >= selected);
                        assert_eq!(occurrence.summary().unwrap(), "Moved following");
                    }
                }
                let reloaded =
                    CalDir::new_from_dir(Arc::new(id.into()), path.into(), id.into(), false)
                        .unwrap();
                assert_eq!(
                    reloaded.occurrences_between(start, end, |_| true).count(),
                    expected as usize
                );
            }
        }
    }

    #[test]
    fn failed_following_calendar_move_preserves_the_original_series() {
        for day in [12, 14] {
            let source = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            let mut state = fixture(source.path(), true);
            add_target(&mut state, &target.path().join("missing"), false);
            let mut edit = UpdateItem::load(
                &state,
                item(Some(&format!("TU2026-10-{day}T09:00:00"))),
                EditScope::Following,
            )
            .unwrap();
            edit.fields.calendar = "target".into();
            edit.fields.summary = "Must not be saved".into();
            assert!(Box::new(edit).execute(&mut state).is_err());
            let file = state.store().file_by_id("edit-me").unwrap();
            assert_eq!(file.directory().as_str(), "test");
            assert_eq!(
                file.component_with(|c| c.rid().is_none())
                    .unwrap()
                    .rrule()
                    .unwrap()
                    .count(),
                Some(5)
            );
            let disk = fs::read_to_string(source.path().join("event.ics")).unwrap();
            assert!(disk.contains("SUMMARY:Original"));
            assert!(disk.contains("COUNT=5"));
            assert!(!disk.contains("Must not be saved"));
        }
    }

    #[test]
    fn rejected_calendar_moves_preserve_the_event() {
        for case in [
            "occurrence",
            "read_only",
            "missing",
            "collision",
            "save_failure",
        ] {
            let source = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            let mut state = fixture(source.path(), true);
            let target_path = if case == "save_failure" {
                target.path().join("missing")
            } else {
                target.path().to_path_buf()
            };
            add_target(&mut state, &target_path, case == "read_only");
            if case == "collision" {
                fs::write(target.path().join("event.ics"), "existing file").unwrap();
            }
            let scope = match case {
                "occurrence" => EditScope::Occurrence,
                _ => EditScope::Series,
            };
            let mut edit =
                UpdateItem::load(&state, item(Some("TU2026-10-13T09:00:00")), scope).unwrap();
            edit.fields.calendar = if case == "missing" {
                "missing"
            } else {
                "target"
            }
            .into();
            edit.fields.summary = "Must not be saved".into();
            assert!(Box::new(edit).execute(&mut state).is_err(), "{case}");
            let file = state
                .store()
                .file_by_id("edit-me")
                .unwrap_or_else(|| panic!("{case}"));
            assert_eq!(file.directory().as_str(), "test", "{case}");
            assert_eq!(
                file.component_with(|c| c.rid().is_none())
                    .unwrap()
                    .summary()
                    .unwrap(),
                "Original",
                "{case}"
            );
            let disk = fs::read_to_string(source.path().join("event.ics")).unwrap();
            assert!(disk.contains("SUMMARY:Original"), "{case}");
            assert!(!disk.contains("Must not be saved"), "{case}");
            if case == "collision" {
                assert_eq!(
                    fs::read_to_string(target.path().join("event.ics")).unwrap(),
                    "existing file"
                );
            }
        }
    }

    #[test]
    fn saves_existing_event_preserving_identity_metadata_and_attendee_responses() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(tmp.path(), false);
        let mut edit = UpdateItem::load(&state, item(None), EditScope::Series).unwrap();
        let stale = edit.clone();
        edit.fields.summary = "Edited".into();
        edit.fields.location = "Office".into();
        assert_eq!(Box::new(edit).execute(&mut state).unwrap(), item(None));
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
        assert!(Box::new(stale).execute(&mut state).is_err());
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
        let mut edit = UpdateItem::load(&state, target.clone(), EditScope::Occurrence).unwrap();
        edit.fields.summary = "Exception".into();
        edit.fields.start = Some("TU2026-10-13T12:00:00".parse().unwrap());
        edit.fields.end = Some("TU2026-10-13T13:00:00".parse().unwrap());
        assert_eq!(Box::new(edit).execute(&mut state).unwrap(), target);
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
        let mut series = UpdateItem::load(&state, item(None), EditScope::Series).unwrap();
        series.fields.description = "Inherited description".into();
        Box::new(series).execute(&mut state).unwrap();
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
            let mut edit = UpdateItem::load(&state, target, EditScope::Following).unwrap();
            assert_eq!(
                edit.fields.recurrence.as_ref().unwrap().count(),
                Some(5 - (day - 12))
            );
            edit.fields.summary = "Following".into();
            let result = Box::new(edit).execute(&mut state).unwrap();
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
        let mut edit = UpdateItem::load(&state, item(None), EditScope::Series).unwrap();
        let original = fs::read(tmp.path().join("event.ics")).unwrap();
        edit.fields.summary.clear();
        assert!(Box::new(edit).execute(&mut state).is_err());
        assert_eq!(fs::read(tmp.path().join("event.ics")).unwrap(), original);
        let edit = UpdateItem::load(&state, item(None), EditScope::Series).unwrap();
        fs::write(tmp.path().join("event.ics"), b"external change").unwrap();
        assert!(Box::new(edit).execute(&mut state).is_err());
        assert_eq!(
            fs::read(tmp.path().join("event.ics")).unwrap(),
            b"external change"
        );
    }
}
