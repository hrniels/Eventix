// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, TimeZone, Timelike};
use chrono_tz::Tz;
use eventix_ical::col::Occurrence;
use eventix_ical::objects::{CalAttendee, CalPartStat, EventLike};
use eventix_state::{PersonalAlarms, Settings};
use once_cell::sync::Lazy;
use std::cmp::Ordering;
use std::{ops::Deref, sync::Mutex};

#[derive(Copy, Clone, Debug)]
pub struct OccurrenceOverlap {
    /// the number of slots next to each other
    pub slots: usize,
    /// our offset within these slots
    pub offset: usize,
    /// how many slots we occupy (in case some next to us are free)
    pub width: usize,
}

impl OccurrenceOverlap {
    pub fn new(slots: usize, offset: usize, width: usize) -> Self {
        Self {
            slots,
            offset,
            width,
        }
    }
}

pub struct DayOccurrence<'a> {
    id: u64,
    inner: Occurrence<'a>,
    overlap: Option<OccurrenceOverlap>,
    partstat: Option<CalPartStat>,
    owner: bool,
    effective_alarms: bool,
}

impl<'a> DayOccurrence<'a> {
    pub fn new_from_settings(
        inner: &Occurrence<'a>,
        settings: &Settings,
        pers_alarms: &PersonalAlarms,
    ) -> Self {
        let (col_settings, cal_settings) = settings.calendar(inner.directory()).unwrap();
        let alarm_type = cal_settings.alarms();
        let user_mail = col_settings.email().map(|e| e.address());
        let partstat = user_mail
            .as_ref()
            .and_then(|addr| inner.attendee_status(addr));
        let owner = inner.is_owned_by(user_mail.as_ref());
        Self::new(
            inner,
            partstat,
            owner,
            pers_alarms.has_alarms(inner, alarm_type),
        )
    }

    pub fn new(
        inner: &Occurrence<'a>,
        partstat: Option<CalPartStat>,
        owner: bool,
        effective_alarms: bool,
    ) -> Self {
        static NEXT_ID: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(0));
        let mut next = NEXT_ID.lock().unwrap();
        let id = *next + 1;
        *next += 1;
        Self {
            id,
            inner: inner.clone(),
            overlap: None,
            partstat,
            owner,
            effective_alarms,
        }
    }

    pub fn occurrences_on<'occ: 'a>(
        occs: &'a [Occurrence<'occ>],
        settings: &Settings,
        pers_alarms: &PersonalAlarms,
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
            .map(|o| DayOccurrence::new_from_settings(o, settings, pers_alarms))
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
        settings: &Settings,
        pers_alarms: &PersonalAlarms,
        date: NaiveDate,
    ) -> Vec<DayOccurrence<'occ>> {
        let mut day_occs = occs
            .iter()
            .filter(|o| match o.occurrence_end() {
                Some(end) => end.date_naive() == date,
                None => false,
            })
            .map(|o| DayOccurrence::new_from_settings(o, settings, pers_alarms))
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

    pub fn is_owner(&self) -> bool {
        self.owner
    }

    pub fn participant_status(&self) -> Option<CalPartStat> {
        self.partstat
    }

    pub fn has_effective_alarms(&self) -> bool {
        self.effective_alarms
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

    pub fn overlap(&self) -> OccurrenceOverlap {
        self.overlap.unwrap()
    }

    pub fn set_overlap(&mut self, overlap: OccurrenceOverlap) {
        self.overlap = Some(overlap);
    }

    pub fn rid_str(&self) -> String {
        match self.inner.rid() {
            Some(rid) => rid.to_string(),
            None if self.inner.is_recurrent() => {
                if let Some(start) = self.inner.occurrence_startdate() {
                    start.to_string()
                } else {
                    String::new()
                }
            }
            None => String::new(),
        }
    }

    pub fn status_class(&self) -> Option<String> {
        if let Some(st) = self.inner.event_status() {
            Some(format!("{st:?}"))
        } else {
            self.inner.todo_status().map(|st| format!("{st:?}"))
        }
    }

    pub fn minute_off(&self, date: NaiveDate) -> u64 {
        if let Some(start) = self.inner.occurrence_start()
            && self.inner.occurrence_starts_on(date)
        {
            return start.hour() as u64 * 60 + start.minute() as u64;
        }
        0
    }

    pub fn minute_duration(&self, date: NaiveDate) -> u64 {
        if self.inner.occurrence_starts_on(date) {
            match self.inner.time_duration() {
                Some(d) => {
                    let start = self.inner.occurrence_start().unwrap();
                    let left = if start.minute() == 0 {
                        (24 - start.hour()) * 60
                    } else {
                        (23 - start.hour()) * 60 + (60 - start.minute())
                    };
                    (left as i64).min(d.num_minutes()) as u64
                }
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
