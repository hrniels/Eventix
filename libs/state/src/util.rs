// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{DateTime, Duration, Local};
use chrono_tz::Tz;
use eventix_ical::{
    col::Occurrence,
    objects::{CalCompType, CalTodoStatus, EventLike},
};

use crate::State;

/// Returns an iterator over incomplete to-do occurrences due within the next `days` days.
///
/// Skips disabled calendars and excludes completed or excluded occurrences.
pub fn due_todos<'a>(state: &'a State, tz: &Tz, days: u32) -> impl Iterator<Item = Occurrence<'a>> {
    let now = Local::now();
    let start = now.with_timezone(tz);
    let end = start + Duration::days(days as i64);

    state
        .store()
        .directories()
        .iter()
        .filter(|s| !state.misc().calendar_disabled(s.id()))
        .flat_map(move |s| s.occurrences_between(start, end, |c| c.ctype() == CalCompType::Todo))
        .filter(|o| {
            !o.is_excluded()
                && o.todo_status().unwrap_or(CalTodoStatus::NeedsAction) != CalTodoStatus::Completed
        })
}

/// Returns an iterator over incomplete to-do occurrences whose due date lies before now.
///
/// Only occurrences whose end falls strictly before the current time are considered overdue.
/// Completed, excluded, and disabled-calendar occurrences are filtered out.
pub fn overdue_todos<'a>(state: &'a State, tz: &Tz) -> impl Iterator<Item = Occurrence<'a>> {
    let now = Local::now();
    let start = now.with_timezone(tz);

    state
        .store()
        .occurrences_between(DateTime::<Tz>::MIN_UTC.with_timezone(tz), start, |c| {
            c.ctype() == CalCompType::Todo
        })
        .filter(move |o| {
            // so far, we got all todos that overlap with this period of time. but we are only
            // interested in the ones that are due before the start and are not complete yet.
            !state.misc().calendar_disabled(o.directory())
                && o.todo_status().unwrap_or(CalTodoStatus::NeedsAction) != CalTodoStatus::Completed
                && o.occurrence_end().unwrap_or(start) < start
                && !o.is_excluded()
        })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use chrono_tz::UTC;
    use eventix_ical::{
        col::{CalDir, CalFile},
        objects::{
            CalComponent, CalDate, CalDateTime, CalDateType, CalTodo, CalTodoStatus, Calendar,
            EventLike, UpdatableEventLike,
        },
    };

    use crate::{State, misc::Misc};
    use eventix_ical::col::CalStore;

    use super::{due_todos, overdue_todos};

    // --- helpers ---

    /// Builds a [`CalTodo`] with the given uid and an optional DUE date (as a UTC datetime).
    ///
    /// Pass a `chrono::DateTime<chrono::Utc>` for `due`. The status defaults to `NeedsAction`
    /// (i.e., `None` on the `CalTodo`).
    fn make_todo(uid: &str, due: Option<chrono::DateTime<chrono::Utc>>) -> CalTodo {
        let mut todo = CalTodo::new(uid);
        if let Some(d) = due {
            todo.set_due(Some(CalDate::DateTime(CalDateTime::Utc(d))));
        }
        todo
    }

    /// Builds an in-memory [`CalFile`] that contains a single TODO component, tagged with the
    /// given directory id so that `Occurrence::directory()` returns the correct value.
    fn make_todo_file(dir_id: &str, todo: CalTodo) -> CalFile {
        let mut cal = Calendar::default();
        cal.add_component(CalComponent::Todo(todo));
        CalFile::new(Arc::new(dir_id.to_string()), PathBuf::default(), cal)
    }

    /// Builds an in-memory [`CalDir`] with the given string id.
    fn make_dir(id: &str) -> CalDir {
        CalDir::new_empty(Arc::new(id.to_string()), PathBuf::default(), id.to_string())
    }

    /// Builds a test [`State`] from a ready-made [`CalStore`] and [`Misc`].
    fn make_state(store: CalStore, misc: Misc) -> State {
        State::new_for_test(store, misc)
    }

    // A DUE date firmly in the past — always overdue regardless of when the test runs.
    fn past_due() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now() - chrono::Duration::days(365)
    }

    // A DUE date firmly in the future — never overdue and always within a generous window.
    fn future_due() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now() + chrono::Duration::days(365)
    }

    // --- due_todos ---

    #[test]
    fn due_todos_includes_todo_with_due_in_window() {
        let todo = make_todo("uid-due", Some(future_due()));
        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));
        // Use large window so `future_due()` falls within it.
        let count = due_todos(&state, &UTC, 365 * 2).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn due_todos_excludes_todo_outside_window() {
        // A todo due in the past is outside the future window — occurrences_between filters it
        // because its DUE date falls strictly before the window start.
        let todo = make_todo("uid-past-outside", Some(past_due()));
        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));
        let count = due_todos(&state, &UTC, 1).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn due_todos_excludes_completed_todo() {
        let mut todo = make_todo("uid-done", Some(future_due()));
        todo.set_status(Some(CalTodoStatus::Completed));
        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));
        let count = due_todos(&state, &UTC, 365 * 2).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn due_todos_excludes_disabled_calendar() {
        let todo = make_todo("uid-disabled", Some(future_due()));
        let mut dir = make_dir("disabled-cal");
        dir.add_file(make_todo_file("disabled-cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        let mut misc = Misc::new(PathBuf::default());
        misc.toggle_calendar(&"disabled-cal".to_string());
        let state = make_state(store, misc);

        let count = due_todos(&state, &UTC, 365 * 2).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn due_todos_excludes_excluded_occurrence() {
        // A recurring todo whose single occurrence falls within the window but has its start date
        // listed in EXDATE must not be returned. For recurring components, occurrences_between
        // yields the occurrence with is_excluded() == true; due_todos must filter it out.
        use eventix_ical::objects::{CalRRule, CalRRuleFreq};
        let start_dt = future_due() - chrono::Duration::hours(1);
        let start_cal_date = CalDate::DateTime(CalDateTime::Utc(start_dt));

        let mut rrule = CalRRule::default();
        rrule.set_frequency(CalRRuleFreq::Daily);
        rrule.set_count(1);

        let mut todo = CalTodo::new("uid-excl");
        todo.set_start(Some(start_cal_date.clone()));
        todo.set_rrule(Some(rrule));

        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        // first check whether it is found when not excluded
        let mut state = make_state(store, Misc::new(PathBuf::default()));
        let count = due_todos(&state, &UTC, 365 * 200).count();
        assert_eq!(count, 1);

        // now exclude it and ensure it's no longer found
        let todo = state
            .store_mut()
            .try_files_by_id_mut("uid-excl")
            .unwrap()
            .component_with_mut(|c| c.uid() == "uid-excl")
            .unwrap();
        todo.toggle_exclude(start_cal_date);

        let count = due_todos(&state, &UTC, 365 * 2).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn due_todos_empty_store_returns_nothing() {
        let state = make_state(CalStore::default(), Misc::new(PathBuf::default()));
        let count = due_todos(&state, &UTC, 7).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn due_todos_needs_action_and_in_process_are_included() {
        // NeedsAction (explicit) and InProcess should both appear in due_todos.
        let mut todo_na = make_todo("uid-na", Some(future_due()));
        todo_na.set_status(Some(CalTodoStatus::NeedsAction));
        let mut todo_ip = make_todo("uid-ip", Some(future_due()));
        todo_ip.set_status(Some(CalTodoStatus::InProcess));

        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo_na));
        dir.add_file(make_todo_file("cal", todo_ip));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));
        let count = due_todos(&state, &UTC, 365 * 2).count();
        assert_eq!(count, 2);
    }

    // --- overdue_todos ---

    #[test]
    fn overdue_todos_includes_todo_with_past_due() {
        let todo = make_todo("uid-overdue", Some(past_due()));
        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));
        let count = overdue_todos(&state, &UTC).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn overdue_todos_excludes_todo_with_future_due() {
        let todo = make_todo("uid-future", Some(future_due()));
        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));
        let count = overdue_todos(&state, &UTC).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn overdue_todos_excludes_completed_todo() {
        let mut todo = make_todo("uid-done-overdue", Some(past_due()));
        todo.set_status(Some(CalTodoStatus::Completed));
        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));
        let count = overdue_todos(&state, &UTC).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn overdue_todos_excludes_disabled_calendar() {
        let todo = make_todo("uid-disabled-overdue", Some(past_due()));
        let mut dir = make_dir("disabled-cal");
        dir.add_file(make_todo_file("disabled-cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        let mut misc = Misc::new(PathBuf::default());
        misc.toggle_calendar(&"disabled-cal".to_string());
        let state = make_state(store, misc);

        let count = overdue_todos(&state, &UTC).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn overdue_todos_multiple_past_todos_all_returned() {
        let todo_a = make_todo("uid-ov-a", Some(past_due()));
        let todo_b = make_todo("uid-ov-b", Some(past_due()));
        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo_a));
        dir.add_file(make_todo_file("cal", todo_b));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));
        let count = overdue_todos(&state, &UTC).count();
        assert_eq!(count, 2);
    }

    #[test]
    fn overdue_todos_empty_store_returns_nothing() {
        let state = make_state(CalStore::default(), Misc::new(PathBuf::default()));
        let count = overdue_todos(&state, &UTC).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn due_todos_and_overdue_are_disjoint() {
        // A past-due todo should appear in overdue_todos but not in due_todos (with a short
        // window); a future todo should appear in due_todos (large window) but not in overdue_todos.
        let todo_past = make_todo("uid-past", Some(past_due()));
        let todo_future = make_todo("uid-future", Some(future_due()));
        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo_past));
        dir.add_file(make_todo_file("cal", todo_future));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));

        // past todo appears in overdue, not in due (short 1-day window)
        let overdue: Vec<_> = overdue_todos(&state, &UTC).collect();
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].uid(), "uid-past");

        // future todo appears in due (large window), not in overdue
        let due: Vec<_> = due_todos(&state, &UTC, 365 * 2).collect();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].uid(), "uid-future");
    }

    #[test]
    fn due_todos_with_date_style_due_included() {
        // DUE as a plain DATE (not datetime) value within the window.
        let future_date = chrono::Utc::now() + chrono::Duration::days(365);
        let naive_date = future_date.naive_utc().date();
        let due_date = CalDate::Date(naive_date, CalDateType::Inclusive);
        let mut todo = CalTodo::new("uid-datedue");
        todo.set_due(Some(due_date));
        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));
        let count = due_todos(&state, &UTC, 365 * 2).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn todo_with_no_due_date_is_excluded() {
        // A todo without any DUE date has no end; occurrences_between skips it.
        let todo = make_todo("uid-noduedate", None);
        let mut dir = make_dir("cal");
        dir.add_file(make_todo_file("cal", todo));
        let mut store = CalStore::default();
        store.add(dir);

        let state = make_state(store, Misc::new(PathBuf::default()));

        let count = due_todos(&state, &UTC, 365 * 2).count();
        assert_eq!(count, 0);
        let count = overdue_todos(&state, &UTC).count();
        assert_eq!(count, 0);
    }
}
