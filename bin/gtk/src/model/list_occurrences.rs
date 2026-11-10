// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Paginated, owned presentation of a series' occurrences.

use std::{collections::BTreeMap, sync::Arc};

use chrono::{DateTime, Duration, LocalResult, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use eventix_ical::{
    col::Occurrence,
    objects::{CalCompType, CalPartStat, CalTodoStatus, EventLike},
};
use eventix_state::{State, actions::items::ItemId};

use crate::model::details::{Details, Response};

/// Stable pagination boundary, including identity when occurrences share a start time.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
    pub start: DateTime<Tz>,
    pub recurrence: String,
}

/// Selects earlier occurrences, or later occurrences with an optional inclusive boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Earlier,
    Later,
    From,
}

/// Compact presentation and action permissions for one occurrence.
#[derive(Clone, Debug)]
pub struct Row {
    pub item: ItemId,
    pub cursor: Cursor,
    pub summary: String,
    pub calendar_name: String,
    pub background: String,
    pub response: Option<Response>,
    pub task: bool,
    pub completed: bool,
    pub cancelled: bool,
    pub excluded: bool,
    pub overwritten: bool,
    pub alarm: bool,
    pub details: Arc<Details>,
}

/// A chronologically ordered page and whether more occurrences exist in its direction.
#[derive(Debug)]
pub struct Page {
    pub rows: Vec<Arc<Row>>,
    pub more: bool,
}

impl Page {
    /// Loads at most `count` occurrences of the selected series, including excluded occurrences.
    ///
    /// Boundaries use effective start times and recurrence identities, avoiding duplicates for
    /// overlapping or rescheduled occurrences. Missing calendars and series return an empty page.
    pub(crate) fn from_state(
        state: &State,
        item: &ItemId,
        cursor: &Cursor,
        dir: Direction,
        count: usize,
    ) -> Self {
        let empty = || Page {
            rows: Vec::new(),
            more: false,
        };
        if count == 0 {
            return empty();
        }
        let Some(file) = state
            .store()
            .directory(&Arc::new(item.calendar.clone()))
            .and_then(|directory| directory.file_by_id(&item.uid))
        else {
            return empty();
        };
        let tz = *state.timezone();
        let (start, end) = if dir == Direction::Earlier {
            (limit(tz, false), cursor.start + Duration::seconds(1))
        } else {
            (cursor.start, limit(tz, true))
        };
        // Keep only a bounded page while scanning backwards. Forward scans stop after the
        // lookahead start-time group, whose identities must be sorted before choosing a cursor.
        let mut selected = BTreeMap::new();
        for occ in file.occurrences_between(start, end, |comp| {
            comp.uid() == &item.uid && comp.is_recurrent()
        }) {
            let Some(key) = occurrence_cursor(&occ) else {
                continue;
            };
            let matches = match dir {
                Direction::Earlier => key < *cursor,
                Direction::Later => key > *cursor,
                Direction::From => key >= *cursor,
            };
            if !matches {
                continue;
            }
            if dir != Direction::Earlier
                && selected.len() > count
                && selected
                    .last_key_value()
                    .is_some_and(|(last, _): (&Cursor, _)| key.start > last.start)
            {
                break;
            }
            selected.insert(key, occ);
            if dir == Direction::Earlier && selected.len() > count + 1 {
                selected.pop_first();
            }
        }
        let more = selected.len() > count;
        if dir == Direction::Earlier && more {
            selected.pop_first();
        }
        let rows = selected
            .into_iter()
            .take(count)
            .filter_map(|(cursor, occ)| {
                let mut details = Details::from_occurrence(state, &occ)?;
                let completed = occ.todo_status() == Some(CalTodoStatus::Completed);
                let excluded = occ.is_excluded();
                details.actions.cancel &= occ.ctype() == CalCompType::Event;
                details.actions.complete &= !completed && !excluded;
                details.actions.postpone &= !completed && !excluded;
                let (collection, settings) = state.settings().calendar(occ.directory())?;
                let email = collection.email().map(|email| email.address());
                let response = email
                    .as_ref()
                    .filter(|email| !occ.is_owned_by(Some(email)))
                    .map(|email| {
                        occ.attendee_status(email)
                            .unwrap_or(CalPartStat::NeedsAction)
                            .into()
                    });
                Some(Arc::new(Row {
                    item: ItemId {
                        calendar: item.calendar.clone(),
                        uid: item.uid.clone(),
                        recurrence: occ.rid().cloned().or_else(|| occ.occurrence_startdate()),
                    },
                    cursor,
                    summary: occ.summary().cloned().unwrap_or_default(),
                    calendar_name: settings.name().clone(),
                    background: settings.bgcolor().clone(),
                    response,
                    task: occ.ctype() == CalCompType::Todo,
                    completed,
                    cancelled: occ.is_cancelled(),
                    excluded,
                    overwritten: occ.is_overwritten(),
                    alarm: state.personal_alarms().has_alarms(&occ, settings.alarms()),
                    details: Arc::new(details),
                }))
            })
            .collect();
        Page { rows, more }
    }
}

fn occurrence_cursor(occ: &Occurrence<'_>) -> Option<Cursor> {
    Some(Cursor {
        start: occ.occurrence_start()?,
        recurrence: occ
            .rid()
            .cloned()
            .or_else(|| occ.occurrence_startdate())?
            .to_string(),
    })
}

fn limit(tz: Tz, maximum: bool) -> DateTime<Tz> {
    let mut naive = if maximum {
        NaiveDateTime::MAX
    } else {
        NaiveDateTime::MIN
    };
    loop {
        if let LocalResult::Single(date) = tz.from_local_datetime(&naive) {
            return date;
        }
        naive += Duration::days(if maximum { -1 } else { 1 });
    }
}
