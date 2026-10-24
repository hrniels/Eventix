// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{fs, io::Write, sync::Arc};

use anyhow::{Context, bail};
use eventix_ical::{
    col::CalFile,
    objects::{
        CalCompType, CalDate, CalTodoStatus, EventLike, PRIORITY_MEDIUM, UpdatableEventLike,
    },
};

use super::ItemId;
use crate::{State, actions::Action};

/// Selects a quick action for a single task occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskAction {
    Complete,
    Postpone { days: u32 },
}

/// Completes or postpones an owned task in a writable calendar.
#[derive(Debug)]
pub struct ChangeTask {
    pub item: ItemId,
    pub action: TaskAction,
}

impl Action for ChangeTask {
    fn affected_items(&self) -> &[ItemId] {
        std::slice::from_ref(&self.item)
    }

    fn execute(self: Box<Self>, state: &mut State) -> anyhow::Result<ItemId> {
        let locale = state.locale();
        let timezone = *state.timezone();
        let invalid = || locale.translate("Could not apply changes").to_string();
        let (collection, _) = state.settings().calendar(&self.item.calendar).context(
            locale
                .translate("Calendar is no longer available")
                .to_string(),
        )?;
        let user_mail = collection.email().map(|email| email.address());
        if collection.is_read_only()
            || matches!(self.action, TaskAction::Postpone { days } if !(1..=7).contains(&days))
        {
            bail!(invalid());
        }

        let directory = Arc::new(self.item.calendar.clone());
        let file = state
            .store_mut()
            .directory_mut(&directory)?
            .file_by_id_mut(&self.item.uid)?;
        let occurrence = file
            .occurrence_by_id(&self.item.uid, self.item.recurrence.as_ref(), &timezone)
            .context(
                locale
                    .translate("Occurrence is no longer available")
                    .to_string(),
            )?;
        if occurrence.ctype() != CalCompType::Todo
            || !occurrence.is_owned_by(user_mail.as_ref())
            || (matches!(self.action, TaskAction::Postpone { .. })
                && occurrence.end_or_due().is_none())
        {
            bail!(invalid());
        }

        // Stage changes so validation and persistence failures leave live state untouched.
        let mut staged = CalFile::new(directory, file.path().clone(), file.calendar().clone());
        staged.change_single(
            &self.item.uid,
            self.item.recurrence.as_ref(),
            &timezone,
            user_mail.as_ref(),
            true,
            |_, component| {
                let todo = component.as_todo_mut().ok_or_else(invalid)?;
                match self.action {
                    TaskAction::Complete => {
                        todo.set_status(Some(CalTodoStatus::Completed));
                        todo.set_percent(Some(100));
                        todo.set_completed(Some(CalDate::now()));
                        // Exchange requires a priority on completed tasks, as in the HTML UI.
                        todo.set_priority(Some(PRIORITY_MEDIUM));
                    }
                    TaskAction::Postpone { days } => {
                        let due = todo.due().ok_or_else(invalid)?;
                        todo.set_due(Some(due.add_days(days)));
                    }
                }
                todo.touch();
                Ok(())
            },
        )?;

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
        objects::{CalDate, CalTodoStatus, DateContext, EventLike, PRIORITY_MEDIUM},
    };

    use super::{Action, ChangeTask, ItemId, TaskAction};
    use crate::{CalendarSettings, CollectionSettings, State, SyncerType};

    fn fixture(path: &Path, body: &str) -> State {
        fs::write(
            path.join("task.ics"),
            format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:task\r\n\
             DTSTART:20261012T090000Z\r\n{body}\r\nSUMMARY:Task\r\n\
             END:VTODO\r\nEND:VCALENDAR\r\n"
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
        collection
            .all_calendars_mut()
            .insert("test".into(), calendar);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        state
    }

    fn command(recurrence: Option<&str>, action: TaskAction) -> Box<ChangeTask> {
        Box::new(ChangeTask {
            item: ItemId {
                calendar: "test".into(),
                uid: "task".into(),
                recurrence: recurrence.map(|r| r.parse().unwrap()),
            },
            action,
        })
    }

    #[test]
    fn completion_persists_all_completion_fields_for_only_the_selected_occurrence() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = fixture(
            tmp.path(),
            "DUE:20261012T100000Z\r\nRRULE:FREQ=DAILY;COUNT=3",
        );
        let action = command(Some("TU2026-10-13T09:00:00"), TaskAction::Complete);
        let item = action.item.clone();
        assert_eq!(action.execute(&mut state).unwrap(), item);
        let dir = CalDir::new_from_dir(
            Arc::new("test".into()),
            tmp.path().into(),
            "Test".into(),
            false,
        )
        .unwrap();
        let file = dir.file_by_id("task").unwrap();
        let completed = file
            .occurrence_by_id("task", item.recurrence.as_ref(), &chrono_tz::UTC)
            .unwrap();
        assert_eq!(completed.todo_status(), Some(CalTodoStatus::Completed));
        let component = file
            .component_with(|c| c.rid() == item.recurrence.as_ref())
            .unwrap();
        let todo = component.as_todo().unwrap();
        assert_eq!(todo.percent(), Some(100));
        assert!(todo.completed().is_some());
        assert_eq!(todo.priority(), Some(PRIORITY_MEDIUM));
        let other: CalDate = "TU2026-10-14T09:00:00".parse().unwrap();
        assert_ne!(
            file.occurrence_by_id("task", Some(&other), &chrono_tz::UTC)
                .unwrap()
                .todo_status(),
            Some(CalTodoStatus::Completed)
        );
    }

    #[test]
    fn postponement_changes_due_only_and_preserves_other_occurrences() {
        for days in [1, 2, 3, 5, 7] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = fixture(
                tmp.path(),
                "DUE:20261012T100000Z\r\nRRULE:FREQ=DAILY;COUNT=3",
            );
            let action = command(Some("TU2026-10-13T09:00:00"), TaskAction::Postpone { days });
            let item = action.execute(&mut state).unwrap();
            let dir = CalDir::new_from_dir(
                Arc::new("test".into()),
                tmp.path().into(),
                "Test".into(),
                false,
            )
            .unwrap();
            let file = dir.file_by_id("task").unwrap();
            let occurrence = file
                .occurrence_by_id("task", item.recurrence.as_ref(), &chrono_tz::UTC)
                .unwrap();
            let due: CalDate = "TU2026-10-13T10:00:00".parse().unwrap();
            let context = DateContext::system();
            assert_eq!(
                occurrence
                    .end_or_due()
                    .map(|date| context.date_to_utc(date, &chrono_tz::UTC)),
                Some(due.add_days(days))
            );
            let start: CalDate = "TU2026-10-13T09:00:00".parse().unwrap();
            assert_eq!(
                occurrence
                    .start()
                    .map(|date| context.date_to_utc(date, &chrono_tz::UTC)),
                Some(start)
            );
            let base = file.component_with(|c| c.rid().is_none()).unwrap();
            assert_eq!(
                base.end_or_due(),
                Some(&"TU2026-10-12T10:00:00".parse().unwrap())
            );
        }
    }

    #[test]
    fn invalid_postponement_and_unowned_tasks_leave_calendar_unchanged() {
        for (body, action) in [
            ("", TaskAction::Postpone { days: 1 }),
            ("DUE:20261012T100000Z", TaskAction::Postpone { days: 8 }),
            ("ORGANIZER:mailto:other@example.org", TaskAction::Complete),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let mut state = fixture(tmp.path(), body);
            let before = fs::read(tmp.path().join("task.ics")).unwrap();
            assert!(command(None, action).execute(&mut state).is_err());
            assert_eq!(fs::read(tmp.path().join("task.ics")).unwrap(), before);
        }
    }
}
