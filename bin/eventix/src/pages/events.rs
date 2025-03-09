use std::sync::Arc;

use chrono::{Duration, Local, NaiveDate};
use ical::objects::CalCompType;

use crate::locale::Locale;
use crate::objects::DayOccurrence;
use crate::state::State;

pub struct Day<'a> {
    pub date: Option<NaiveDate>,
    pub occurrences: Vec<DayOccurrence<'a>>,
}

pub struct Events<'a> {
    pub days: Vec<Day<'a>>,
}

impl<'a> Events<'a> {
    pub fn new(state: &'a State, locale: &Arc<dyn Locale + Send + Sync>) -> Events<'a> {
        Self::new_with_days(state, locale, 7)
    }

    pub fn new_with_days(
        state: &'a State,
        locale: &Arc<dyn Locale + Send + Sync>,
        days: u32,
    ) -> Events<'a> {
        let timezone = locale.timezone();

        let now = Local::now();
        let start = now.with_timezone(locale.timezone());
        let end = start + Duration::days(days as i64);

        let next_ev_occs = state
            .store()
            .directories()
            .iter()
            .filter(|s| !state.settings().calendar_disabled(s.id()))
            .flat_map(move |s| {
                s.occurrences_between(start, end, |c| c.ctype() == CalCompType::Event)
            })
            .filter(|o| !o.is_excluded())
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

        Self { days }
    }
}
