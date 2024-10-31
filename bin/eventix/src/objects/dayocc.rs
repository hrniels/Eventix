use std::cmp::Ordering;
use std::{ops::Deref, sync::Mutex};

use chrono::{NaiveDate, TimeZone};
use chrono_tz::Tz;
use ical::col::Occurrence;
use ical::objects::{CalTodoStatus, EventLike};
use once_cell::sync::Lazy;

pub struct DayOccurrence<'a> {
    id: u64,
    inner: Occurrence<'a>,
}

impl<'a> DayOccurrence<'a> {
    pub fn new(inner: &Occurrence<'a>) -> Self {
        static NEXT_ID: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(0));
        let mut next = NEXT_ID.lock().unwrap();
        let id = *next + 1;
        *next += 1;
        Self {
            id,
            inner: inner.clone(),
        }
    }

    pub fn occurrences_on<'occ: 'a>(
        occs: &'a [Occurrence<'occ>],
        date: NaiveDate,
        timezone: &Tz,
    ) -> Vec<DayOccurrence<'occ>> {
        let day_start = timezone
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .unwrap();
        let day_end = timezone
            .from_local_datetime(&date.and_hms_opt(23, 59, 59).unwrap())
            .unwrap();

        let mut day_occs = occs
            .iter()
            .filter(|o| o.overlaps(day_start, day_end))
            .map(DayOccurrence::new)
            .collect::<Vec<_>>();
        day_occs.sort_by(|a, b| match (a.is_all_day(), b.is_all_day()) {
            (true, true) | (false, false) => a.occurrence_start().cmp(&b.occurrence_start()),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
        });
        day_occs
    }

    pub fn due_occurrences<'occ: 'a>(
        occs: &'a [Occurrence<'occ>],
        date: NaiveDate,
    ) -> Vec<DayOccurrence<'occ>> {
        let mut day_occs = occs
            .iter()
            .filter(|o| match o.end_or_due() {
                Some(end) => end.as_naive_date() == date,
                None => false,
            })
            .map(DayOccurrence::new)
            .collect::<Vec<_>>();
        day_occs.sort_by(|a, b| match (a.is_all_day(), b.is_all_day()) {
            (true, true) | (false, false) => a.end_or_due().cmp(&b.end_or_due()),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
        });
        day_occs
    }

    pub fn unplanned_occurrences<'occ: 'a>(
        occs: &'a [Occurrence<'occ>],
    ) -> Vec<DayOccurrence<'occ>> {
        let mut unplanned_occs = occs
            .iter()
            .filter(|o| {
                o.end_or_due().is_none()
                    && o.todo_status().unwrap_or(CalTodoStatus::NeedsAction)
                        != CalTodoStatus::Completed
            })
            .map(DayOccurrence::new)
            .collect::<Vec<_>>();
        unplanned_occs.sort_by(|a, b| match (a.created(), b.created()) {
            (Some(ac), Some(bc)) => ac.cmp(bc),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        });
        unplanned_occs
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn uid_html(&self) -> String {
        self.inner
            .uid()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect()
    }

    pub fn rid_html(&self) -> String {
        match self.inner.rid() {
            Some(rid) => rid.to_string(),
            None => self
                .inner
                .occurrence_start()
                .to_utc()
                .format("%Y%m%dT%H%M%SZ")
                .to_string(),
        }
    }

    pub fn status_class(&self) -> Option<String> {
        self.inner.event_status().map(|st| format!("{:?}", st))
    }
}

impl<'a> Deref for DayOccurrence<'a> {
    type Target = Occurrence<'a>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
