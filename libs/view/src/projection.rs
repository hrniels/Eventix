// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared occurrence selection and display-timezone projection.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Timelike};
use chrono_tz::Tz;
use eventix_ical::{
    col::Occurrence,
    objects::{CalCompType, EventLike},
    util,
};
use eventix_state::State;

use crate::{Event, OccurrenceId, details};

pub(crate) fn events_between(
    state: &State,
    start: NaiveDate,
    end: NaiveDate,
) -> Vec<(Occurrence<'_>, Event)> {
    let tz = state.timezone();
    let start = util::resolve_local_time(tz, start.and_hms_opt(0, 0, 0).unwrap());
    let end = util::resolve_local_time(tz, end.and_hms_opt(0, 0, 0).unwrap());
    state
        .store()
        .directories()
        .iter()
        .filter(|dir| !state.misc().calendar_disabled(dir.id()))
        .flat_map(|dir| dir.occurrences_between(start, end, |c| c.ctype() == CalCompType::Event))
        .filter(|occ| !occ.is_excluded())
        .filter_map(|occ| {
            let details = Arc::new(details::Details::load(state, &occ)?);
            let start = occ.occurrence_start()?.with_timezone(tz);
            let end = occ.occurrence_end().unwrap_or(start).with_timezone(tz);
            let (_, settings) = state.settings().calendar(occ.directory())?;
            let event = Event {
                id: OccurrenceId {
                    calendar: occ.directory().to_string(),
                    uid: occ.uid().to_string(),
                    recurrence: occ.rid().map(ToString::to_string).unwrap_or_else(|| {
                        if occ.is_recurrent() {
                            occ.occurrence_startdate()
                                .map(|d| d.to_string())
                                .unwrap_or_default()
                        } else {
                            String::new()
                        }
                    }),
                },
                summary: occ.summary().cloned().unwrap_or_default(),
                calendar: settings.name().clone(),
                foreground: settings.fgcolor().clone(),
                background: settings.bgcolor().clone(),
                details,
                start,
                end,
                all_day: occ.is_all_day(),
                cancelled: occ.is_cancelled(),
                recurrent: occ.is_recurrent(),
                exception: occ.is_overwritten(),
                alarm: state.personal_alarms().has_alarms(&occ, settings.alarms()),
            };
            Some((occ, event))
        })
        .collect()
}

pub(crate) fn event_order(a: &Event, b: &Event) -> std::cmp::Ordering {
    (a.start, &a.id.calendar, &a.summary, &a.id).cmp(&(b.start, &b.id.calendar, &b.summary, &b.id))
}

pub(crate) fn day_minutes(
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    date: NaiveDate,
) -> Option<(u32, u32)> {
    let midnight = util::resolve_local_time(&start.timezone(), date.and_hms_opt(0, 0, 0)?);
    let next = util::resolve_local_time(&start.timezone(), date.succ_opt()?.and_hms_opt(0, 0, 0)?);
    if start >= next || end < midnight || (end == midnight && end != start) {
        return None;
    }
    let from = if start < midnight {
        0
    } else {
        start.hour() * 60 + start.minute()
    };
    let to = if end >= next {
        1440
    } else {
        end.hour() * 60 + end.minute()
    };
    // A backward clock change can reverse the displayed start and end times.
    Some((from.min(to), from.max(to)))
}
