use std::sync::Arc;

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use chrono_tz::Tz;
use ical::col::{CalStore, Occurrence};
use ical::objects::{CalCompType, CalTodoStatus, EventLike};
use tokio::sync::MutexGuard;

use crate::locale::Locale;
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
    pub fn new(
        store: &'a MutexGuard<'_, CalStore>,
        disabled: &'a MutexGuard<'_, Vec<String>>,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> Tasks<'a> {
        Self::new_with_days(store, disabled, locale, 21)
    }

    pub fn new_with_days(
        store: &'a MutexGuard<'_, CalStore>,
        disabled: &'a MutexGuard<'_, Vec<String>>,
        locale: &Arc<dyn Locale + Send + Sync>,
        days: u32,
    ) -> Tasks<'a> {
        let timezone = locale.timezone();

        let now = Local::now();
        let start = now.with_timezone(locale.timezone());
        let end = start + Duration::days(days as i64);

        let mut next_td_occs = store
            .sources()
            .iter()
            .filter(|s| !disabled.contains(s.id()))
            .flat_map(move |s| {
                s.filtered_occurrences_within(start, end, |c| c.ctype() == CalCompType::Todo)
            })
            .filter(|o| {
                o.todo_status().unwrap_or(CalTodoStatus::NeedsAction) != CalTodoStatus::Completed
            })
            .collect::<Vec<_>>();

        let overdue_tds = store
            .filtered_occurrences_within(
                DateTime::<Tz>::MIN_UTC.with_timezone(timezone),
                start,
                |c| c.ctype() == CalCompType::Todo,
            )
            .filter(|o| {
                // so far, we got all todos that overlap with this period of time. but we are only
                // interested in the ones that are due before the start and are not complete yet.
                o.todo_status().unwrap_or(CalTodoStatus::NeedsAction) != CalTodoStatus::Completed
                    && o.occurrence_end().unwrap_or(start) < start
            });

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
            let day_occs = DayOccurrence::due_occurrences(&next_td_occs, cur_date);
            if !day_occs.is_empty() {
                days.push(Day {
                    date: Some(cur_date),
                    occurrences: day_occs,
                });
            }

            cur_date += Duration::days(1);
        }

        let unplanned_occs = store
            .items()
            .filter(|s| !disabled.contains(s.source()))
            .flat_map(|i| i.components().iter().map(|c| (i.source(), c)))
            .filter(|(_source, c)| {
                c.ctype() == CalCompType::Todo
                    && !c.is_recurrent()
                    && c.end_or_due().is_none()
                    && c.as_todo()
                        .unwrap()
                        .status()
                        .unwrap_or(CalTodoStatus::NeedsAction)
                        != CalTodoStatus::Completed
            })
            .map(|(source, c)| {
                Occurrence::new(
                    source.clone(),
                    c,
                    c.start().map(|d| d.as_start_with_tz(timezone)),
                    None,
                )
            })
            .collect::<Vec<_>>();

        let mut unplanned_occs = unplanned_occs
            .iter()
            .map(DayOccurrence::new)
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
