// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Now-relative upcoming events and incomplete tasks, matching the HTML sidebar.

use std::{collections::BTreeMap, sync::Arc};

use chrono::{DateTime, Duration, NaiveDate};
use chrono_tz::Tz;
use eventix_ical::{
    col::Occurrence,
    objects::{CalCompType, CalDate, CalTodoStatus, EventLike},
    util,
};
use eventix_locale::{Locale, TimeFlags};
use eventix_state::State;

use crate::{
    OccurrenceId,
    details::{Details, Response},
    editor::EditorCalendar,
};

/// One read-only sidebar placement, including tasks without start or due dates.
#[derive(Clone, Debug)]
pub struct SidebarRow {
    pub id: OccurrenceId,
    pub summary: String,
    pub calendar: String,
    pub foreground: String,
    pub background: String,
    pub details: Arc<Details>,
    pub start: Option<DateTime<Tz>>,
    pub due: Option<DateTime<Tz>>,
    pub full_day: bool,
    pub time: String,
    pub task: bool,
    pub overdue: bool,
    pub due_today: bool,
    pub cancelled: bool,
    pub recurrent: bool,
    pub exception: bool,
    pub alarm: bool,
    pub response: Option<Response>,
}

impl SidebarRow {
    /// Loads an occurrence for details even when it is outside the upcoming task list.
    pub fn load(state: &State, item: &eventix_state::actions::items::ItemId) -> Option<Self> {
        let file = state.store().file_by_id(&item.uid)?;
        let occ = file.occurrence_by_id(&item.uid, item.recurrence.as_ref(), state.timezone())?;
        let now = chrono::Utc::now().with_timezone(state.timezone());
        row(state, &occ, now.date_naive(), now)
    }
}

/// A dated group, or the final unplanned-task group when `date` is absent.
#[derive(Clone, Debug)]
pub struct SidebarGroup {
    pub date: Option<NaiveDate>,
    pub rows: Vec<SidebarRow>,
}

/// Owned upcoming lists independent of the currently browsed calendar period.
pub struct SidebarSnapshot {
    pub events: Vec<SidebarGroup>,
    pub tasks: Vec<SidebarGroup>,
    pub task_calendars: Vec<EditorCalendar>,
    pub last_task_calendar: Option<String>,
    pub locale: Arc<dyn Locale + Send + Sync>,
}

impl SidebarSnapshot {
    /// Loads seven days of events, overdue and next-21-day tasks, and unplanned tasks.
    ///
    /// Uses `now` in the configured timezone. Like HTML, the final date of each window is
    /// excluded from display; completed, excluded and disabled-calendar tasks are omitted.
    pub fn load(state: &State, now: DateTime<Tz>) -> Self {
        let tz = state.timezone();
        let now = now.with_timezone(tz);
        let events = event_groups(state, now);
        let mut tasks = dated_task_groups(state, now);
        if let Some(unplanned) = unplanned_task_group(state, now) {
            tasks.push(unplanned);
        }
        Self {
            events,
            tasks,
            task_calendars: EditorCalendar::load(state, CalCompType::Todo),
            last_task_calendar: state.misc().last_calendar(CalCompType::Todo).cloned(),
            locale: state.locale(),
        }
    }
}

fn event_groups(state: &State, now: DateTime<Tz>) -> Vec<SidebarGroup> {
    let event_end = now + Duration::days(7);
    let occurrences: Vec<_> = state
        .store()
        .directories()
        .iter()
        .filter(|dir| !state.misc().calendar_disabled(dir.id()))
        .flat_map(|dir| {
            dir.occurrences_between(now, event_end, |c| c.ctype() == CalCompType::Event)
        })
        .filter(|occ| !occ.is_excluded())
        .collect();

    let mut events = Vec::new();
    for offset in 0..(event_end.date_naive() - now.date_naive()).num_days() {
        let date = now.date_naive() + Duration::days(offset);
        let rows = event_rows_on(state, &occurrences, date, now);
        if !rows.is_empty() {
            events.push(SidebarGroup {
                date: Some(date),
                rows,
            });
        }
    }

    events
}

fn event_rows_on(
    state: &State,
    occurrences: &[Occurrence<'_>],
    date: NaiveDate,
    now: DateTime<Tz>,
) -> Vec<SidebarRow> {
    let tz = state.timezone();
    let start = util::resolve_local_time(tz, date.and_hms_opt(0, 0, 0).unwrap());
    let end = util::resolve_local_time(tz, date.and_hms_opt(23, 59, 59).unwrap());
    let mut rows: Vec<_> = occurrences
        .iter()
        .filter(|occ| occ.overlaps(start, end))
        .filter_map(|occ| row(state, occ, date, now))
        .collect();
    sort_rows(&mut rows, false);
    rows
}

/// Groups overdue and upcoming tasks by due date, skipping dates without tasks.
fn dated_task_groups(state: &State, now: DateTime<Tz>) -> Vec<SidebarGroup> {
    let tz = state.timezone();
    let task_end = now + Duration::days(21);

    // One query covers both overdue and upcoming tasks with a single, explicit clock.
    let mut tasks: BTreeMap<NaiveDate, Vec<SidebarRow>> = BTreeMap::new();
    for occ in state
        .store()
        .directories()
        .iter()
        .filter(|dir| !state.misc().calendar_disabled(dir.id()))
        .flat_map(|dir| {
            dir.occurrences_between(DateTime::<Tz>::MIN_UTC.with_timezone(tz), task_end, |c| {
                c.ctype() == CalCompType::Todo
            })
        })
        .filter(|occ| !occ.is_excluded() && occ.todo_status() != Some(CalTodoStatus::Completed))
    {
        if let Some(due) = occ.occurrence_end() {
            let date = due.with_timezone(tz).date_naive();
            if date < task_end.date_naive()
                && let Some(row) = row(state, &occ, date, now)
            {
                tasks.entry(date).or_default().push(row);
            }
        }
    }

    tasks
        .into_iter()
        .map(|(date, mut rows)| {
            sort_rows(&mut rows, true);
            SidebarGroup {
                date: Some(date),
                rows,
            }
        })
        .collect()
}

fn unplanned_task_group(state: &State, now: DateTime<Tz>) -> Option<SidebarGroup> {
    let tz = state.timezone();

    let mut unplanned = Vec::new();
    for file in state
        .store()
        .files()
        .filter(|file| !state.misc().calendar_disabled(file.directory()))
    {
        for component in file.components().iter().filter(|c| {
            c.ctype() == CalCompType::Todo
                && !c.is_recurrent()
                && c.end_or_due().is_none()
                && c.as_todo()
                    .is_some_and(|t| t.status() != Some(CalTodoStatus::Completed))
        }) {
            let ctx = file.calendar().date_context();
            let occ = Occurrence::new(
                file.directory().clone(),
                component,
                component.start().map(|d| ctx.date(d).resolved_start(tz)),
                None,
                false,
            );
            if let Some(row) = row(state, &occ, now.date_naive(), now) {
                unplanned.push((occ.created().cloned(), row));
            }
        }
    }

    unplanned.sort_by_key(|(created, _)| created.clone());
    if unplanned.is_empty() {
        None
    } else {
        Some(SidebarGroup {
            date: None,
            rows: unplanned.into_iter().map(|(_, row)| row).collect(),
        })
    }
}

fn sort_rows(rows: &mut [SidebarRow], tasks: bool) {
    rows.sort_by(|a, b| {
        (
            !a.full_day,
            if tasks { a.due } else { a.start },
            &a.id.calendar,
            &a.summary,
        )
            .cmp(&(
                !b.full_day,
                if tasks { b.due } else { b.start },
                &b.id.calendar,
                &b.summary,
            ))
    });
}

fn row(
    state: &State,
    occ: &Occurrence<'_>,
    date: NaiveDate,
    now: DateTime<Tz>,
) -> Option<SidebarRow> {
    let tz = state.timezone();
    let locale = state.locale();
    let (collection, calendar) = state.settings().calendar(occ.directory())?;
    let task = occ.ctype() == CalCompType::Todo;
    let start = occ.occurrence_start().map(|d| d.with_timezone(tz));
    let due = occ.occurrence_end().map(|d| d.with_timezone(tz));
    let full_day = occ.is_all_day() || occ.is_all_day_on(date);
    let time = row_time(occ, date, full_day, &*locale);
    let mail = collection.email().map(|e| e.address());
    Some(SidebarRow {
        id: occurrence_id(occ),
        summary: occ.summary().cloned().unwrap_or_default(),
        calendar: calendar.name().clone(),
        foreground: calendar.fgcolor().clone(),
        background: calendar.bgcolor().clone(),
        details: Arc::new(Details::load(state, occ)?),
        start,
        due,
        full_day,
        time,
        task,
        overdue: task && due.is_some_and(|d| d < now),
        due_today: task && due.is_some_and(|d| d >= now && d.date_naive() == now.date_naive()),
        cancelled: occ.is_cancelled(),
        recurrent: occ.is_recurrent(),
        exception: occ.is_overwritten(),
        alarm: state.personal_alarms().has_alarms(occ, calendar.alarms()),
        response: if occ.is_owned_by(mail.as_ref()) {
            None
        } else {
            mail.as_ref()
                .and_then(|m| occ.attendee_status(m))
                .map(Response::from)
        },
    })
}

fn occurrence_id(occ: &Occurrence<'_>) -> OccurrenceId {
    let recurrence = occ.rid().map(ToString::to_string).unwrap_or_else(|| {
        if occ.is_recurrent() {
            occ.occurrence_startdate()
                .map(|d| d.to_string())
                .unwrap_or_default()
        } else {
            String::new()
        }
    });
    OccurrenceId {
        calendar: occ.directory().to_string(),
        uid: occ.uid().to_string(),
        recurrence,
    }
}

fn row_time(occ: &Occurrence<'_>, date: NaiveDate, full_day: bool, locale: &dyn Locale) -> String {
    if occ.ctype() == CalCompType::Todo {
        task_time(occ, locale)
    } else if full_day {
        String::new()
    } else {
        event_time(occ, date, locale)
    }
}

fn task_time(occ: &Occurrence<'_>, locale: &dyn Locale) -> String {
    match (occ.occurrence_enddate(), occ.occurrence_end()) {
        (Some(CalDate::DateTime(..)), Some(due)) => {
            locale.fmt_time(&due.with_timezone(locale.timezone()), TimeFlags::Short)
        }
        _ => String::new(),
    }
}

fn event_time(occ: &Occurrence<'_>, date: NaiveDate, locale: &dyn Locale) -> String {
    let (time, prefix) = if occ.occurrence_starts_on(date) {
        (
            occ.occurrence_start(),
            if occ.occurrence_ends_on(date) {
                ""
            } else {
                "→ "
            },
        )
    } else {
        (occ.occurrence_end(), "← ")
    };
    time.map(|time| {
        format!(
            "{prefix}{}",
            locale.fmt_time(&time.with_timezone(locale.timezone()), TimeFlags::Short)
        )
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::TimeZone;
    use chrono_tz::Europe::Berlin;
    use eventix_ical::col::{CalDir, CalFile};
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};

    use super::SidebarSnapshot;

    fn state(components: &[&str]) -> (tempfile::TempDir, State) {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let mut state = State::new_with_timezone(Arc::new(xdg), Some(Berlin)).unwrap();
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: tmp.path().to_string_lossy().into_owned(),
        });
        let mut settings = CalendarSettings::default();
        settings.set_enabled(true);
        settings.set_name("Work".into());
        collection
            .all_calendars_mut()
            .insert("cal".into(), settings);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        let id = Arc::new("cal".to_string());
        let mut dir = CalDir::new_empty(id.clone(), tmp.path().into(), "Work".into(), false);
        for (index, component) in components.iter().enumerate() {
            let calendar = format!("BEGIN:VCALENDAR\nVERSION:2.0\n{component}\nEND:VCALENDAR\n")
                .parse()
                .unwrap();
            dir.add_file(CalFile::new(
                id.clone(),
                tmp.path().join(format!("{index}.ics")),
                calendar,
            ))
            .unwrap();
        }
        state.store_mut().add(dir);
        (tmp, state)
    }

    #[test]
    fn events_match_html_order_continuations_exclusions_and_window() {
        let (_tmp, state) = state(&[
            "BEGIN:VEVENT\nUID:past\nDTSTART:20261005T060000Z\nDTEND:20261005T070000Z\nEND:VEVENT",
            "BEGIN:VEVENT\nUID:all\nDTSTART;VALUE=DATE:20261005\nDTEND;VALUE=DATE:20261006\nEND:VEVENT",
            "BEGIN:VEVENT\nUID:timed\nDTSTART:20261005T090000Z\nDTEND:20261005T100000Z\nEND:VEVENT",
            "BEGIN:VEVENT\nUID:long\nDTSTART:20261005T100000Z\nDTEND:20261007T130000Z\nEND:VEVENT",
            "BEGIN:VEVENT\nUID:timezone\nDTSTART:20261005T233000Z\nDTEND:20261006T003000Z\nEND:VEVENT",
            "BEGIN:VEVENT\nUID:series\nDTSTART:20261005T120000Z\nDTEND:20261005T130000Z\nRRULE:FREQ=DAILY;COUNT=3\nEXDATE:20261006T120000Z\nEND:VEVENT",
            "BEGIN:VEVENT\nUID:boundary\nDTSTART:20261012T060000Z\nDTEND:20261012T070000Z\nEND:VEVENT",
            "BEGIN:VEVENT\nUID:cancelled\nDTSTART:20261005T150000Z\nDTEND:20261005T160000Z\nSTATUS:CANCELLED\nEND:VEVENT",
        ]);
        let now = Berlin.with_ymd_and_hms(2026, 10, 5, 10, 0, 0).unwrap();
        let snapshot = SidebarSnapshot::load(&state, now);
        let rows = &snapshot.events[0].rows;
        assert_eq!(
            rows.iter()
                .map(|row| row.id.uid.as_str())
                .collect::<Vec<_>>(),
            ["all", "timed", "long", "series", "cancelled"]
        );
        assert!(rows[0].full_day && rows[0].time.is_empty());
        assert_eq!(rows[1].time, "11:00");
        assert_eq!(rows[2].time, "→ 12:00");
        assert!(rows[4].cancelled);
        assert_eq!(
            snapshot.events[1].rows.len(),
            2,
            "excluded recurrence is hidden"
        );
        assert!(snapshot.events[1].rows[0].full_day);
        assert_eq!(snapshot.events[1].rows[1].id.uid, "timezone");
        assert_eq!(snapshot.events[1].rows[1].time, "01:30");
        assert_eq!(snapshot.events[2].rows[0].time, "← 15:00");
        assert!(
            snapshot
                .events
                .iter()
                .all(|group| group.rows.iter().all(|row| row.id.uid != "boundary"))
        );
    }

    #[test]
    fn tasks_include_old_overdue_date_only_and_unplanned_but_skip_completed_and_disabled() {
        let (_tmp, mut state) = state(&[
            "BEGIN:VTODO\nUID:old\nDUE:20200101T120000Z\nEND:VTODO",
            "BEGIN:VTODO\nUID:today\nDUE:20261005T180000Z\nSTATUS:IN-PROCESS\nEND:VTODO",
            "BEGIN:VTODO\nUID:date\nDUE;VALUE=DATE:20261005\nEND:VTODO",
            "BEGIN:VTODO\nUID:completed\nDUE:20261005T170000Z\nSTATUS:COMPLETED\nEND:VTODO",
            "BEGIN:VTODO\nUID:boundary\nDUE:20261026T060000Z\nEND:VTODO",
            "BEGIN:VTODO\nUID:unplanned-new\nCREATED:20260102T000000Z\nEND:VTODO",
            "BEGIN:VTODO\nUID:unplanned-old\nDTSTART;VALUE=DATE:20260101\nCREATED:20260101T000000Z\nEND:VTODO",
            "BEGIN:VTODO\nUID:unplanned-done\nSTATUS:COMPLETED\nEND:VTODO",
            "BEGIN:VTODO\nUID:series\nDTSTART:20261006T100000Z\nDUE:20261006T110000Z\nRRULE:FREQ=DAILY;COUNT=2\nEXDATE:20261007T100000Z\nEND:VTODO",
        ]);
        let now = Berlin.with_ymd_and_hms(2026, 10, 5, 10, 0, 0).unwrap();
        let snapshot = SidebarSnapshot::load(&state, now);
        assert_eq!(snapshot.tasks.len(), 4);
        assert_eq!(snapshot.tasks[0].rows[0].id.uid, "old");
        assert!(snapshot.tasks[0].rows[0].overdue);
        let today = &snapshot.tasks[1].rows;
        assert_eq!(
            today
                .iter()
                .map(|row| row.id.uid.as_str())
                .collect::<Vec<_>>(),
            ["date", "today"]
        );
        assert!(today[0].time.is_empty());
        assert!(
            !today[0].overdue && today[0].due_today,
            "DATE due includes the whole day"
        );
        assert_eq!(today[1].time, "20:00");
        assert!(today[1].due_today && today[1].details.status.is_some());
        assert_eq!(snapshot.tasks[2].rows[0].id.uid, "series");
        assert_eq!(snapshot.tasks[2].rows.len(), 1);
        let unplanned = &snapshot.tasks[3];
        assert_eq!(unplanned.date, None);
        assert_eq!(
            unplanned
                .rows
                .iter()
                .map(|row| row.id.uid.as_str())
                .collect::<Vec<_>>(),
            ["unplanned-old", "unplanned-new"]
        );
        assert!(unplanned.rows.iter().all(|row| row.due.is_none()));
        assert!(unplanned.rows[1].start.is_none());
        state.misc_mut().toggle_calendar(&"cal".to_string());
        let hidden = SidebarSnapshot::load(&state, now);
        assert!(hidden.tasks.is_empty() && hidden.events.is_empty());
    }
}
