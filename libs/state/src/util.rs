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
