use std::cmp::Ordering;
use std::{ops::Deref, sync::Mutex};

use chrono::{NaiveDate, TimeZone, Timelike};
use chrono_tz::Tz;
use ical::col::Occurrence;
use ical::objects::{CalAttendee, EventLike};
use once_cell::sync::Lazy;

pub struct DayOccurrence<'a> {
    id: u64,
    inner: Occurrence<'a>,
    overlap: Option<(usize, usize)>,
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
            overlap: None,
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
        day_occs.sort_by_key(|i| {
            (
                !(i.is_all_day() || i.is_all_day_on(date)),
                i.occurrence_start(),
                i.directory().clone(),
                i.summary().cloned(),
            )
        });
        day_occs
    }

    pub fn due_occurrences<'occ: 'a>(
        occs: &'a [Occurrence<'occ>],
        date: NaiveDate,
    ) -> Vec<DayOccurrence<'occ>> {
        let mut day_occs = occs
            .iter()
            .filter(|o| match o.occurrence_end() {
                Some(end) => end.date_naive() == date,
                None => false,
            })
            .map(DayOccurrence::new)
            .collect::<Vec<_>>();
        day_occs.sort_by_key(|i| {
            (
                !(i.is_all_day() || i.is_all_day_on(date)),
                i.end_or_due().cloned(),
                i.directory().clone(),
                i.summary().cloned(),
            )
        });
        day_occs
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn attendees_sorted(&self) -> Vec<&CalAttendee> {
        if let Some(atts) = self.attendees() {
            let mut att = atts.iter().collect::<Vec<_>>();
            att.sort_by(|a, b| match (a.common_name(), b.common_name()) {
                (Some(cn1), Some(cn2)) => cn1.cmp(cn2),
                _ => Ordering::Equal,
            });
            att
        } else {
            vec![]
        }
    }

    pub fn overlap_count(&self) -> usize {
        self.overlap.unwrap().0
    }

    pub fn overlap_off(&self) -> usize {
        self.overlap.unwrap().1
    }

    pub fn set_overlap(&mut self, overlap: (usize, usize)) {
        self.overlap = Some(overlap);
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
            None => {
                if let Some(start) = self.inner.occurrence_start() {
                    start.to_utc().format("%Y%m%dT%H%M%SZ").to_string()
                } else {
                    String::new()
                }
            }
        }
    }

    pub fn status_class(&self) -> Option<String> {
        if let Some(st) = self.inner.event_status() {
            Some(format!("{:?}", st))
        } else {
            self.inner.todo_status().map(|st| format!("{:?}", st))
        }
    }

    pub fn minute_off(&self, date: NaiveDate) -> u64 {
        if let Some(start) = self.inner.occurrence_start() {
            if self.inner.occurrence_starts_on(date) {
                return start.hour() as u64 * 60 + start.minute() as u64;
            }
        }
        0
    }

    pub fn minute_duration(&self, date: NaiveDate) -> u64 {
        if self.inner.occurrence_starts_on(date) {
            match self.inner.duration() {
                Some(d) => d.num_minutes() as u64,
                None => 0,
            }
        } else {
            let end = self.inner.occurrence_end().unwrap();
            end.hour() as u64 * 60 + end.minute() as u64
        }
    }
}

impl<'a> Deref for DayOccurrence<'a> {
    type Target = Occurrence<'a>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
