use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use chrono_tz::Tz;
use ical::col::Occurrence;
use ical::objects::{CalCompType, CalTodoStatus, EventLike};
use std::sync::Arc;

use crate::locale::Locale;
use crate::objects::DayOccurrence;
use crate::state::State;

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

        let mut next_td_occs = state
            .store()
            .directories()
            .iter()
            .filter(|s| !state.misc().calendar_disabled(s.id()))
            .flat_map(move |s| {
                s.occurrences_between(start, end, |c| c.ctype() == CalCompType::Todo)
            })
            .filter(|o| {
                !o.is_excluded()
                    && o.todo_status().unwrap_or(CalTodoStatus::NeedsAction)
                        != CalTodoStatus::Completed
            })
            .collect::<Vec<_>>();

        let overdue_tds = state
            .store()
            .occurrences_between(
                DateTime::<Tz>::MIN_UTC.with_timezone(timezone),
                start,
                |c| c.ctype() == CalCompType::Todo,
            )
            .filter(|o| {
                // so far, we got all todos that overlap with this period of time. but we are only
                // interested in the ones that are due before the start and are not complete yet.
                o.todo_status().unwrap_or(CalTodoStatus::NeedsAction) != CalTodoStatus::Completed
                    && o.occurrence_end().unwrap_or(start) < start
                    && !o.is_excluded()
            });

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
                    c.start().map(|d| d.as_start_with_tz(timezone)),
                    None,
                    // non-recurrent occurrences are never excluded
                    false,
                )
            })
            .collect::<Vec<_>>();

        let mut unplanned_occs = unplanned_occs
            .iter()
            .map(|o| {
                let alarm_type = settings.calendar(o.directory()).unwrap().alarms();
                DayOccurrence::new(o, pers_alarms.has_alarms(o, alarm_type))
            })
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
