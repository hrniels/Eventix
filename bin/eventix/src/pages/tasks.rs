use std::sync::Arc;

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use chrono_tz::Tz;
use ical::col::{CalStore, Id};
use ical::objects::{CalCompType, CalComponent, CalTodoStatus, EventLike};
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
    store: &'a MutexGuard<'a, CalStore>,
}

impl<'a> Tasks<'a> {
    pub fn new(
        store: &'a MutexGuard<'_, CalStore>,
        locale: &Arc<dyn Locale + Send + Sync>,
    ) -> Tasks<'a> {
        Self::new_with_days(store, locale, 7)
    }

    pub fn new_with_days(
        store: &'a MutexGuard<'_, CalStore>,
        locale: &Arc<dyn Locale + Send + Sync>,
        days: u32,
    ) -> Tasks<'a> {
        let timezone = locale.timezone();

        let now = Local::now();
        let start = now.with_timezone(locale.timezone());
        let end = start + Duration::days(days as i64);

        let mut next_td_occs = store
            .filtered_occurrences_within(start, end, |c| c.ctype() == CalCompType::Todo)
            .collect::<Vec<_>>();

        let overdue_tds = store
            .filtered_occurrences_within(
                DateTime::<Tz>::MIN_UTC.with_timezone(timezone),
                start,
                |c| match c {
                    CalComponent::Todo(td) if td.due().is_some() => {
                        td.status().unwrap_or(CalTodoStatus::NeedsAction)
                            != CalTodoStatus::Completed
                    }
                    _ => false,
                },
            )
            .filter(|o| {
                // so far, we got all todos that overlap with this period of time. but we are only
                // interested in the ones that are due before the start.
                o.end_or_due()
                    .map(|e| e.as_end_with_tz(timezone))
                    .unwrap_or(start)
                    < start
            });

        let mut days = Vec::new();

        next_td_occs.extend(overdue_tds);
        let mut cur_date = next_td_occs
            .iter()
            .map(|o| {
                o.end_or_due()
                    .map(|e| e.as_end_with_tz(timezone))
                    .unwrap_or(start)
            })
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

        let unplanned_occs = DayOccurrence::unplanned_occurrences(&next_td_occs);
        if !unplanned_occs.is_empty() {
            days.push(Day {
                date: None,
                occurrences: unplanned_occs,
            });
        }

        Self {
            days,
            store,
            today: Utc::now().with_timezone(timezone).date_naive(),
        }
    }

    pub fn calendar_name(&self, source: Id) -> &str {
        self.store.source(source).unwrap().name()
    }
}
