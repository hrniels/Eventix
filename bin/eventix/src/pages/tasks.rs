// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{Duration, Local, NaiveDate, Utc};
use eventix_ical::col::Occurrence;
use eventix_ical::objects::{CalCompType, CalTodoStatus, EventLike};
use eventix_locale::Locale;
use eventix_state::State;
use std::sync::Arc;

use crate::objects::DayOccurrence;

pub struct Day<'a> {
    pub date: Option<NaiveDate>,
    pub occurrences: Vec<DayOccurrence<'a>>,
}

pub struct Tasks<'a> {
    pub days: Vec<Day<'a>>,
    pub today: NaiveDate,
}

impl<'a> Tasks<'a> {
    pub fn new(state: &'a State, locale: &Arc<dyn Locale + Send + Sync>) -> Tasks<'a> {
        Self::new_with_days(state, locale, 21)
    }

    pub fn new_with_days(
        state: &'a State,
        locale: &Arc<dyn Locale + Send + Sync>,
        days: u32,
    ) -> Tasks<'a> {
        let timezone = locale.timezone();

        let now = Local::now();
        let start = now.with_timezone(locale.timezone());
        let end = start + Duration::days(days as i64);

        let mut next_td_occs =
            eventix_state::util::due_todos(state, locale.timezone(), days).collect::<Vec<_>>();

        let overdue_tds = eventix_state::util::overdue_todos(state, locale.timezone());

        let settings = state.settings();
        let pers_alarms = state.personal_alarms();

        let mut days = Vec::new();

        next_td_occs.extend(overdue_tds);
        let mut cur_date = next_td_occs
            .iter()
            .map(|o| o.occurrence_end().unwrap_or(start))
            .min()
            .unwrap_or(start)
            .date_naive();
        let end_date = end.date_naive();
        while cur_date < end_date {
            let day_occs =
                DayOccurrence::due_occurrences(&next_td_occs, settings, pers_alarms, cur_date);
            if !day_occs.is_empty() {
                days.push(Day {
                    date: Some(cur_date),
                    occurrences: day_occs,
                });
            }

            cur_date += Duration::days(1);
        }

        let unplanned_occs = state
            .store()
            .files()
            .filter(|s| !state.misc().calendar_disabled(s.directory()))
            .flat_map(|i| i.components().iter().map(|c| (i.directory(), c)))
            .filter(|(_dir, c)| {
                c.ctype() == CalCompType::Todo
                    && !c.is_recurrent()
                    && c.end_or_due().is_none()
                    && c.as_todo()
                        .unwrap()
                        .status()
                        .unwrap_or(CalTodoStatus::NeedsAction)
                        != CalTodoStatus::Completed
            })
            .map(|(dir, c)| {
                Occurrence::new(
                    dir.clone(),
                    c,
                    c.start()
                        .map(|d| d.as_start_with_tz(timezone).fixed_offset().into()),
                    None,
                    // non-recurrent occurrences are never excluded
                    false,
                )
            })
            .collect::<Vec<_>>();

        let mut unplanned_occs = unplanned_occs
            .iter()
            .map(|o| DayOccurrence::new_from_settings(o, settings, pers_alarms))
            .collect::<Vec<_>>();
        unplanned_occs.sort_by_key(|i| i.created().cloned());
        if !unplanned_occs.is_empty() {
            days.push(Day {
                date: None,
                occurrences: unplanned_occs,
            });
        }

        Self {
            days,
            today: Utc::now().with_timezone(timezone).date_naive(),
        }
    }
}
