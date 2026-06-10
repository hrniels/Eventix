// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, NaiveTime, Timelike};
use chrono_tz::Tz;
use eventix_ical::col::Occurrence;
use eventix_ical::objects::{CalAttendee, CalPartStat, EventLike};
use eventix_ical::util;
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
    read_only: bool,
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
            col_settings.is_read_only(),
            pers_alarms.has_alarms(inner, alarm_type),
        )
    }

    pub fn new(
        inner: &Occurrence<'a>,
        partstat: Option<CalPartStat>,
        owner: bool,
        read_only: bool,
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
            read_only,
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
        let day_start = util::resolve_local_time(timezone, date.and_hms_opt(0, 0, 0).unwrap());
        let day_end = util::resolve_local_time(timezone, date.and_hms_opt(23, 59, 59).unwrap());

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
                i.occurrence_end(),
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

    pub fn is_read_only(&self) -> bool {
        self.read_only
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

    /// Returns the hour component of this occurrence's start time, or 0 if the start is unknown.
    pub fn occurrence_start_hour(&self) -> u32 {
        self.inner.occurrence_start().map(|s| s.hour()).unwrap_or(0)
    }

    /// Returns the minute component of this occurrence's start time, or 0 if the start is unknown.
    pub fn occurrence_start_min(&self) -> u32 {
        self.inner
            .occurrence_start()
            .map(|s| s.minute())
            .unwrap_or(0)
    }

    /// Returns the hour component of this occurrence's end time, or 0 if the end is unknown.
    pub fn occurrence_end_hour(&self) -> u32 {
        self.inner.occurrence_end().map(|e| e.hour()).unwrap_or(0)
    }

    /// Returns the minute component of this occurrence's end time, or 0 if the end is unknown.
    pub fn occurrence_end_min(&self) -> u32 {
        self.inner.occurrence_end().map(|e| e.minute()).unwrap_or(0)
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
        let Some(start) = self.inner.occurrence_start() else {
            return 0;
        };
        let end = self.inner.occurrence_end().unwrap_or(start);

        let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        if self.inner.occurrence_starts_on(date) {
            let start_time = start.time();
            // if the occurrence ends on this day, but end-time is midnight, we use 23:59:59
            let end_time = if self.inner.occurrence_ends_on(date) && end.time() != midnight {
                end.time()
            } else {
                NaiveTime::from_hms_opt(23, 59, 59).unwrap()
            };
            if end_time >= start_time {
                (end_time - start_time).num_minutes() as u64
            } else {
                0
            }
        } else {
            // we do not call this if the event is running the full day on `date`
            assert!(self.inner.occurrence_ends_on(date));
            let end_time = end.time();
            if end_time == midnight {
                24 * 60
            } else {
                (end_time - midnight).num_minutes() as u64
            }
        }
    }
}

impl<'a> Deref for DayOccurrence<'a> {
    type Target = Occurrence<'a>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};
    use chrono_tz::Tz;
    use eventix_ical::objects::{CalComponent, CalEvent, ResolvedDateTime};
    use std::sync::Arc;

    fn dir() -> Arc<String> {
        Arc::new("test-dir".to_string())
    }

    fn timed_occ<'a>(
        start: chrono::DateTime<Tz>,
        end: chrono::DateTime<Tz>,
        display_tz: Tz,
    ) -> DayOccurrence<'a> {
        static COMP: Lazy<CalComponent> = Lazy::new(|| {
            let ev = CalEvent::new("uid");
            CalComponent::Event(ev)
        });
        let occ = Occurrence::new_in_tz(
            dir(),
            &COMP,
            Some(ResolvedDateTime::from(start.fixed_offset())),
            Some(ResolvedDateTime::from(end.fixed_offset())),
            false,
            display_tz,
        );
        DayOccurrence::new(&occ, None, false, false, false)
    }

    #[test]
    fn minute_duration_dst_gap() {
        let tz: Tz = "Europe/Berlin".parse().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 3, 30).unwrap();

        // 01:30 to 03:30. Gap is 02:00 -> 03:00.
        let start = tz.with_ymd_and_hms(2025, 3, 30, 1, 30, 0).unwrap();
        let end = tz.with_ymd_and_hms(2025, 3, 30, 3, 30, 0).unwrap();
        let docc = timed_occ(start, end, tz);
        assert_eq!(docc.minute_duration(date), 2 * 60);
    }

    #[test]
    fn minute_duration_dst_fold() {
        let tz: Tz = "Europe/Berlin".parse().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 10, 26).unwrap();

        // 01:30 to 03:30. Fold is 03:00 -> 02:00.
        let start = tz.with_ymd_and_hms(2025, 10, 26, 1, 30, 0).unwrap();
        let end = tz.with_ymd_and_hms(2025, 10, 26, 3, 30, 0).unwrap();
        let docc = timed_occ(start, end, tz);
        assert_eq!(docc.minute_duration(date), 2 * 60);
    }

    #[test]
    fn minute_duration_multi_day_dst_gap() {
        let tz: Tz = "Europe/Berlin".parse().unwrap();
        let mar29 = NaiveDate::from_ymd_opt(2025, 3, 29).unwrap();
        let mar30 = NaiveDate::from_ymd_opt(2025, 3, 30).unwrap();

        let start = tz.with_ymd_and_hms(2025, 3, 29, 22, 0, 0).unwrap();
        let end = tz.with_ymd_and_hms(2025, 3, 30, 4, 0, 0).unwrap();
        let docc = timed_occ(start, end, tz);

        assert_eq!(docc.minute_duration(mar29), 2 * 60 - 1); // until end of day
        assert_eq!(docc.minute_duration(mar30), 4 * 60);
    }
}
