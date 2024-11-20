use std::sync::{Arc, MutexGuard};

use chrono::{Duration, Local, NaiveDate};
use ical::col::{CalStore, Id};
use ical::objects::CalCompType;

use crate::locale::Locale;
use crate::objects::DayOccurrence;

pub struct Day<'a> {
    pub date: Option<NaiveDate>,
    pub occurrences: Vec<DayOccurrence<'a>>,
}

pub struct Events<'a> {
    pub days: Vec<Day<'a>>,
    store: &'a MutexGuard<'a, CalStore>,
}

impl<'a> Events<'a> {
    pub fn new(
        store: &'a MutexGuard<'_, CalStore>,
        locale: &Arc<dyn Locale + Send + Sync>,
        days: u32,
    ) -> Events<'a> {
        let timezone = locale.timezone();

        let now = Local::now();
        let start = now.with_timezone(locale.timezone());
        let end = start + Duration::days(days as i64);

        let next_ev_occs = store
            .filtered_occurrences_within(start, end, |c| c.ctype() == CalCompType::Event)
            .collect::<Vec<_>>();

        let mut days = Vec::new();
        let mut cur_date = start.date_naive();
        let end_date = end.date_naive();
        while cur_date < end_date {
            let day_occs = DayOccurrence::occurrences_on(&next_ev_occs, cur_date, timezone);
            if !day_occs.is_empty() {
                days.push(Day {
                    date: Some(cur_date),
                    occurrences: day_occs,
                });
            }

            cur_date += Duration::days(1);
        }

        Self { days, store }
    }

    pub fn calendar_name(&self, source: Id) -> &str {
        self.store.source(source).unwrap().name()
    }
}
