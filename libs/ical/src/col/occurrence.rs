// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use chrono::{DateTime, Duration, NaiveDate};
use chrono_tz::Tz;

use crate::objects::{
    CalAlarm, CalAttendee, CalCompType, CalComponent, CalDate, CalDuration, CalEventStatus,
    CalOrganizer, CalRRule, CalTodoStatus, CompDateType, EventLike,
};
use crate::parser::{Property, PropertyProducer};
use crate::util;

macro_rules! ctype_method {
    ($self:expr, $ctype:tt, $method:tt) => {
        match $self.overwrite {
            Some(o) if o.$ctype().and_then(|td| td.$method()).is_some() => {
                o.$ctype().and_then(|td| td.$method())
            }
            _ => $self.base.$ctype().and_then(|td| td.$method()),
        }
    };
}

/// An occurrence of an event/TODO.
///
/// If the event/TODO is non-recurrent, its occurrence is simply the single date on that it is
/// scheduled. Otherwise, the event/TODO has potentially many occurrences defined by the recurrence
/// rule and potential overwrites.
///
/// This struct contains both the base component and the overwritten component. When accessing
/// properties, the value of the base component will be delivered by default. If this property has
/// been overwritten, the value of the overwritten component will be delivered.
///
/// Occurrences do not necessarily have both a start and an end date. If the component they are
/// derived from do not have an end date, for example, the occurrence will not have one either.
/// However, recurrent components need to have a start date.
///
/// Occurrences can also be excluded. Most APIs will deliver these occurrences so that the caller
/// can decide whether to ignore them or not. The method [`Self::is_excluded`] can be used to check
/// it.
#[derive(Debug, Clone)]
pub struct Occurrence<'c> {
    dir: Arc<String>,
    start: Option<DateTime<Tz>>,
    end: Option<DateTime<Tz>>,
    base: &'c CalComponent,
    overwrite: Option<&'c CalComponent>,
    excluded: bool,
}

impl<'c> Occurrence<'c> {
    /// Creates a new occurrence at given start/end date.
    ///
    /// The `dir` specifies the directory the component lives in and `base` specifies the base
    /// component. `start` and `end` specify when this occurrence takes place. `excluded` specifies
    /// whether this occurrence has been excluded at the base component.
    pub fn new(
        dir: Arc<String>,
        base: &'c CalComponent,
        start: Option<DateTime<Tz>>,
        end: Option<DateTime<Tz>>,
        excluded: bool,
    ) -> Self {
        Self {
            dir,
            start,
            end,
            base,
            overwrite: None,
            excluded,
        }
    }

    /// Creates a new occurrence for a single date (either start or end).
    ///
    /// The `dir` specifies the directory the component lives in and `base` specifies the base
    /// component. `ty` specifies which date is present, whereas `date` specifies the date itself.
    /// `excluded` specifies whether this occurrence has been excluded at the base component.
    pub fn new_single(
        dir: Arc<String>,
        base: &'c CalComponent,
        ty: CompDateType,
        date: DateTime<Tz>,
        excluded: bool,
    ) -> Self {
        Self::new(
            dir,
            base,
            if ty == CompDateType::Start {
                Some(date)
            } else {
                None
            },
            if ty == CompDateType::EndOrDue {
                Some(date)
            } else {
                None
            },
            excluded,
        )
    }

    /// Returns the timezone used for this occurrence.
    ///
    /// Note that may be None in case neither the start or the end of the occurrence is known.
    pub fn tz(&self) -> Option<Tz> {
        match self.start {
            Some(start) => Some(start.timezone()),
            None => self.end.map(|end| end.timezone()),
        }
    }

    /// Returns the directory in which the underlying component lives.
    pub fn directory(&self) -> &Arc<String> {
        &self.dir
    }

    /// Returns the component type (event/TODO).
    pub fn ctype(&self) -> CalCompType {
        self.base.ctype()
    }

    /// Returns the base component.
    pub fn base(&self) -> &CalComponent {
        self.base
    }

    /// Returns true if the component has been overwritten.
    pub fn is_overwritten(&self) -> bool {
        self.overwrite.is_some()
    }

    /// Sets the given component as the overwrite for the contained base component.
    ///
    /// If an overwrite is set, its non-`None` properties will overwrite the properties of the base
    /// component.
    ///
    /// Note also that the start of this occurrence will be taken from the overwrite in case it has
    /// specified a start date.
    pub fn set_overwrite(&mut self, overwrite: &'c CalComponent) {
        self.overwrite = Some(overwrite);
        if let Some(ostart) = overwrite.start() {
            self.start = Some(ostart.as_start_with_tz(&self.tz().unwrap()));
        }
        if let Some(oend) = overwrite.end_or_due() {
            self.end = Some(oend.as_end_with_tz(&self.tz().unwrap()));
        }
    }

    /// Returns true if this occurrence has been excluded.
    pub fn is_excluded(&self) -> bool {
        self.excluded
    }

    /// Returns true if this occurrence has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        match self.ctype() {
            CalCompType::Todo => {
                self.todo_status().unwrap_or(CalTodoStatus::InProcess) == CalTodoStatus::Cancelled
            }
            CalCompType::Event => {
                self.event_status().unwrap_or(CalEventStatus::Tentative)
                    == CalEventStatus::Cancelled
            }
        }
    }

    /// Returns the [`CalEventStatus`] in case this is a [`CalCompType::Event`].
    pub fn event_status(&self) -> Option<CalEventStatus> {
        ctype_method!(self, as_event, status)
    }

    /// Returns the [`CalTodoStatus`] in case this is a [`CalCompType::Todo`].
    pub fn todo_status(&self) -> Option<CalTodoStatus> {
        ctype_method!(self, as_todo, status)
    }

    /// Returns the percentage of completion in case this is a [`CalCompType::Todo`].
    pub fn todo_percent(&self) -> Option<u8> {
        ctype_method!(self, as_todo, percent)
    }

    /// Returns the completion date in case this is a [`CalCompType::Todo`].
    pub fn todo_completed(&self) -> Option<&CalDate> {
        ctype_method!(self, as_todo, completed)
    }

    /// Returns the start of this occurrence (if known).
    pub fn occurrence_start(&self) -> Option<DateTime<Tz>> {
        self.start
    }

    /// Returns the start of this occurrence (if known) as a [`CalDate`].
    pub fn occurrence_startdate(&self) -> Option<CalDate> {
        self.start.map(|start| {
            if self.is_all_day() {
                CalDate::Date(start.date_naive(), self.ctype().into())
            } else {
                start.into()
            }
        })
    }

    /// Returns the end of this occurrence (if known).
    pub fn occurrence_end(&self) -> Option<DateTime<Tz>> {
        match self.end {
            Some(end) => Some(end),
            None => self.time_duration().map(|d| self.start.unwrap() + d),
        }
    }

    /// Returns the end of this occurrence (if known) as a [`CalDate`].
    pub fn occurrence_enddate(&self) -> Option<CalDate> {
        self.occurrence_end().and_then(|e| {
            if self.is_all_day() {
                let date = match self.ctype() {
                    CalCompType::Todo => e.date_naive(),
                    CalCompType::Event => e.date_naive().succ_opt()?,
                };
                Some(CalDate::Date(date, self.ctype().into()))
            } else {
                Some(e.into())
            }
        })
    }

    /// Returns whether this occurrence starts on that given date.
    pub fn occurrence_starts_on(&self, date: NaiveDate) -> bool {
        match self.occurrence_start() {
            Some(start) => start.date_naive() == date,
            None => false,
        }
    }

    // Returns whether this occurrence ends on that given date.
    pub fn occurrence_ends_on(&self, date: NaiveDate) -> bool {
        self.occurrence_end()
            .map(|end| end.date_naive() == date)
            .unwrap_or(false)
    }

    /// Returns whether this occurrence lasts the complete day on the given date.
    pub fn is_all_day_on(&self, date: NaiveDate) -> bool {
        let end = self
            .occurrence_end()
            .map(|e| e.date_naive())
            .unwrap_or(date);
        match self.occurrence_start() {
            Some(start) => date > start.date_naive() && date < end,
            None => false,
        }
    }

    /// Returns whether this occurrence overlaps with given time period.
    pub fn overlaps(&self, start: DateTime<Tz>, end: DateTime<Tz>) -> bool {
        if let Some(ostart) = self.start {
            util::date_ranges_overlap(ostart, self.occurrence_end().unwrap_or(ostart), start, end)
        } else if let Some(oend) = self.occurrence_end() {
            oend >= start && oend < end
        } else {
            false
        }
    }
}

macro_rules! occ_or_base {
    ($self:tt, $method:tt) => {
        match $self.overwrite {
            Some(overwrite) => overwrite.$method(),
            _ => $self.base.$method(),
        }
    };
}

macro_rules! occ_or_base_opt {
    ($self:tt, $method:tt) => {
        match $self.overwrite {
            Some(overwrite) if overwrite.$method().is_some() => overwrite.$method(),
            _ => $self.base.$method(),
        }
    };
}

impl PropertyProducer for Occurrence<'_> {
    fn to_props(&self) -> Vec<Property> {
        let props = vec![];
        props
    }
}

impl EventLike for Occurrence<'_> {
    fn ctype(&self) -> CalCompType {
        self.base.ctype()
    }

    fn uid(&self) -> &String {
        occ_or_base!(self, uid)
    }

    fn stamp(&self) -> &CalDate {
        occ_or_base!(self, stamp)
    }

    fn created(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, created)
    }

    fn last_modified(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, last_modified)
    }

    fn start(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, start)
    }

    fn end_or_due(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, end_or_due)
    }

    fn duration(&self) -> Option<&CalDuration> {
        occ_or_base_opt!(self, duration)
    }

    fn summary(&self) -> Option<&String> {
        occ_or_base_opt!(self, summary)
    }

    fn description(&self) -> Option<&String> {
        occ_or_base_opt!(self, description)
    }

    fn location(&self) -> Option<&String> {
        occ_or_base_opt!(self, location)
    }

    fn categories(&self) -> Option<&[String]> {
        occ_or_base_opt!(self, categories)
    }

    fn organizer(&self) -> Option<&CalOrganizer> {
        occ_or_base_opt!(self, organizer)
    }

    fn attendees(&self) -> Option<&[CalAttendee]> {
        occ_or_base_opt!(self, attendees)
    }

    fn exdates(&self) -> &[CalDate] {
        self.base.exdates()
    }

    fn alarms(&self) -> Option<&[CalAlarm]> {
        occ_or_base_opt!(self, alarms)
    }

    fn rrule(&self) -> Option<&CalRRule> {
        occ_or_base_opt!(self, rrule)
    }

    fn rid(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, rid)
    }

    fn priority(&self) -> Option<u8> {
        occ_or_base_opt!(self, priority)
    }

    fn time_duration(&self) -> Option<Duration> {
        if let Some(duration) = self.duration() {
            return Some(**duration);
        }

        let (start, end): (CalDate, Option<CalDate>) = match self.overwrite {
            Some(overwrite) => match (
                self.base.start(),
                self.base.end_or_due(),
                overwrite.start(),
                overwrite.end_or_due(),
            ) {
                // if both are overwritten, use them for the duration
                (_, _, Some(ostart), Some(oend)) => (ostart.clone(), Some(oend.clone())),
                // if just one or none is overwritten, it's the duration of the base component
                (Some(_), Some(_), _, _) => return self.base.time_duration(),
                // otherwise, we simply don't know the duration
                _ => return None,
            },
            None => (self.base.start()?.clone(), self.base.end_or_due().cloned()),
        };

        // ensure that we start day-aligned if either start or end is all-day
        let start = if self.is_all_day() && !matches!(start, CalDate::Date(..)) {
            CalDate::Date(start.as_naive_date(), self.ctype().into())
        } else {
            start.clone()
        };

        let tz = Tz::UTC;
        end.map(|end| end.as_end_with_tz(&tz) - start.as_start_with_tz(&tz))
    }
}

/// An occurrence with a due alarm.
#[derive(Debug)]
pub struct AlarmOccurrence<'o> {
    occ: Occurrence<'o>,
    alarm: CalAlarm,
}

impl<'o> AlarmOccurrence<'o> {
    /// Creates a new instance for the given alarm that is associated with the given occurrence.
    pub fn new(occ: Occurrence<'o>, alarm: CalAlarm) -> Self {
        Self { occ, alarm }
    }

    /// Returns a reference to the occurrence for which the alarm should trigger
    pub fn occurrence(&self) -> &Occurrence<'o> {
        &self.occ
    }

    pub(crate) fn occurrence_mut(&mut self) -> &mut Occurrence<'o> {
        &mut self.occ
    }

    /// Returns the alarm that should trigger
    pub fn alarm(&self) -> &CalAlarm {
        &self.alarm
    }

    /// Returns the first alarm date of this occurrence, if it has an alarm.
    pub fn alarm_date(&self) -> Option<DateTime<Tz>> {
        self.alarm.trigger_date(
            self.occ.occurrence_start(),
            self.occ.occurrence_end(),
            self.occ.tz(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, NaiveDate, TimeZone, Utc};
    use chrono_tz::{Tz, UTC};

    use crate::objects::{
        CalAction, CalAlarm, CalAttendee, CalCompType, CalComponent, CalDate, CalDateTime,
        CalDateType, CalEvent, CalEventStatus, CalOrganizer, CalRRule, CalRelated, CalTodo,
        CalTodoStatus, CalTrigger, CompDateType, EventLike, UpdatableEventLike,
    };
    use crate::parser::{LineReader, Property, PropertyProducer};

    use super::{AlarmOccurrence, Occurrence};

    fn dir() -> Arc<String> {
        Arc::new("test-dir".to_string())
    }

    fn utc(year: i32, month: u32, day: u32, h: u32, m: u32, s: u32) -> chrono::DateTime<Tz> {
        UTC.with_ymd_and_hms(year, month, day, h, m, s).unwrap()
    }

    fn allday_event(uid: &str, date: NaiveDate) -> CalEvent {
        let mut ev = CalEvent::new(uid);
        ev.set_start(Some(CalDate::Date(date, CalCompType::Event.into())));
        ev.set_end(Some(CalDate::Date(
            date.succ_opt().unwrap(),
            CalCompType::Event.into(),
        )));
        ev
    }

    fn allday_todo(uid: &str, date: NaiveDate) -> CalTodo {
        let mut td = CalTodo::new(uid);
        td.set_start(Some(CalDate::Date(date, CalCompType::Todo.into())));
        td.set_due(Some(CalDate::Date(date, CalCompType::Todo.into())));
        td
    }

    fn timed_event(uid: &str, start_dt: chrono::DateTime<Tz>) -> CalEvent {
        let mut ev = CalEvent::new(uid);
        ev.set_start(Some(start_dt.into()));
        ev.set_end(Some((start_dt + Duration::hours(1)).into()));
        ev
    }

    /// Verifies `directory`, `ctype`, `base`, `is_overwritten`, `is_excluded`,
    /// `occurrence_start`, and the `start`-branch of `tz` in a single test.
    #[test]
    fn simple_getters() {
        let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let ev = CalEvent::new("uid-simple");
        let comp = CalComponent::Event(ev);
        let start = utc(2024, 6, 15, 9, 0, 0);

        let occ = Occurrence::new(dir(), &comp, Some(start), None, false);

        assert_eq!(occ.directory().as_str(), "test-dir");
        assert_eq!(occ.ctype(), CalCompType::Event);
        assert_eq!(occ.base().uid(), "uid-simple");
        assert!(!occ.is_overwritten());
        assert!(!occ.is_excluded());
        assert_eq!(occ.occurrence_start(), Some(start));
        assert_eq!(occ.tz(), Some(UTC));

        // is_excluded = true path
        let occ_excl = Occurrence::new(dir(), &comp, Some(start), None, true);
        assert!(occ_excl.is_excluded());

        // Verify date is unused — suppress unused-import warning
        let _ = date;
    }

    /// Verifies `tz()` returns the end timezone when `start` is `None`.
    #[test]
    fn tz_from_end_when_no_start() {
        let ev = CalEvent::new("uid-tz");
        let comp = CalComponent::Event(ev);
        let end = utc(2024, 3, 10, 12, 0, 0);

        let occ = Occurrence::new_single(dir(), &comp, CompDateType::EndOrDue, end, false);

        // start is None, end is Some — tz() must come from end
        assert_eq!(occ.tz(), Some(UTC));
        assert_eq!(occ.occurrence_start(), None);
    }

    /// Verifies `occurrence_startdate` and `occurrence_enddate` for timed (non-all-day) events.
    #[test]
    fn occurrence_startdate_and_enddate_datetime() {
        let start = utc(2024, 5, 20, 10, 0, 0);
        let end = utc(2024, 5, 20, 11, 0, 0);
        let ev = timed_event("uid-timed", start);
        let comp = CalComponent::Event(ev);

        let occ = Occurrence::new(dir(), &comp, Some(start), Some(end), false);

        let startdate = occ.occurrence_startdate().unwrap();
        let expected_start = CalDate::DateTime(CalDateTime::Timezone(
            start.naive_local(),
            start.timezone().name().to_string(),
        ));
        assert_eq!(startdate, expected_start);

        let enddate = occ.occurrence_enddate().unwrap();
        let expected_end = CalDate::DateTime(CalDateTime::Timezone(
            end.naive_local(),
            end.timezone().name().to_string(),
        ));
        assert_eq!(enddate, expected_end);
    }

    /// Verifies all-day event `occurrence_startdate` and `occurrence_enddate` (end advances +1 day).
    #[test]
    fn occurrence_startdate_and_enddate_allday_event() {
        let date = NaiveDate::from_ymd_opt(2024, 7, 4).unwrap();
        let ev = allday_event("uid-allday", date);
        let comp = CalComponent::Event(ev);

        // For all-day events the start is stored as UTC midnight of that date
        let start_dt = UTC.with_ymd_and_hms(2024, 7, 4, 0, 0, 0).unwrap();
        let end_dt = UTC.with_ymd_and_hms(2024, 7, 5, 0, 0, 0).unwrap();
        let occ = Occurrence::new(dir(), &comp, Some(start_dt), Some(end_dt), false);

        let startdate = occ.occurrence_startdate().unwrap();
        assert!(
            matches!(startdate, CalDate::Date(d, CalDateType::Exclusive) if d == date),
            "expected start date {date}, got {startdate:?}"
        );

        // For events the enddate is advanced by one day (exclusive semantics)
        let enddate = occ.occurrence_enddate().unwrap();
        let expected_end = date.succ_opt().unwrap().succ_opt().unwrap(); // 2024-07-06
        assert!(
            matches!(enddate, CalDate::Date(d, CalDateType::Exclusive) if d == expected_end),
            "expected end date {expected_end}, got {enddate:?}"
        );
    }

    /// Verifies all-day TODO `occurrence_enddate` does NOT advance the date by one day.
    #[test]
    fn occurrence_enddate_allday_todo() {
        let date = NaiveDate::from_ymd_opt(2024, 8, 1).unwrap();
        let td = allday_todo("uid-todo", date);
        let comp = CalComponent::Todo(td);

        let start_dt = UTC.with_ymd_and_hms(2024, 8, 1, 0, 0, 0).unwrap();
        let end_dt = UTC.with_ymd_and_hms(2024, 8, 1, 0, 0, 0).unwrap();
        let occ = Occurrence::new(dir(), &comp, Some(start_dt), Some(end_dt), false);

        let enddate = occ.occurrence_enddate().unwrap();
        // For TODOs the due date is inclusive — same date, no advancement
        assert!(
            matches!(enddate, CalDate::Date(d, CalDateType::Inclusive) if d == date),
            "expected due date {date}, got {enddate:?}"
        );
    }

    /// Verifies `occurrence_starts_on` and `occurrence_ends_on` for hit, miss, and None-start cases.
    #[test]
    fn occurrence_starts_on_and_ends_on() {
        let start = utc(2024, 9, 10, 8, 0, 0);
        let end = utc(2024, 9, 10, 9, 0, 0);
        let ev = CalEvent::new("uid-on");
        let comp = CalComponent::Event(ev);

        let occ = Occurrence::new(dir(), &comp, Some(start), Some(end), false);

        let on_day = NaiveDate::from_ymd_opt(2024, 9, 10).unwrap();
        let other_day = NaiveDate::from_ymd_opt(2024, 9, 11).unwrap();

        assert!(occ.occurrence_starts_on(on_day));
        assert!(!occ.occurrence_starts_on(other_day));
        assert!(occ.occurrence_ends_on(on_day));
        assert!(!occ.occurrence_ends_on(other_day));

        // No start: occurrence_starts_on must return false
        let no_start = Occurrence::new(dir(), &comp, None, None, false);
        assert!(!no_start.occurrence_starts_on(on_day));
        // No end (and no duration): occurrence_ends_on must return false
        assert!(!no_start.occurrence_ends_on(on_day));
    }

    /// Verifies `is_all_day_on` for mid-span, boundary, and None-start cases.
    #[test]
    fn is_all_day_on() {
        // Multi-day event: starts 2024-10-01, ends 2024-10-04
        let start = UTC.with_ymd_and_hms(2024, 10, 1, 0, 0, 0).unwrap();
        let end = UTC.with_ymd_and_hms(2024, 10, 4, 0, 0, 0).unwrap();
        let ev = CalEvent::new("uid-span");
        let comp = CalComponent::Event(ev);
        let occ = Occurrence::new(dir(), &comp, Some(start), Some(end), false);

        // Mid-span date: the occurrence spans fully over this day
        let mid = NaiveDate::from_ymd_opt(2024, 10, 2).unwrap();
        assert!(occ.is_all_day_on(mid));

        // Start date: not strictly after start — returns false
        let start_day = NaiveDate::from_ymd_opt(2024, 10, 1).unwrap();
        assert!(!occ.is_all_day_on(start_day));

        // End date boundary: not strictly before end — returns false
        let end_day = NaiveDate::from_ymd_opt(2024, 10, 4).unwrap();
        assert!(!occ.is_all_day_on(end_day));

        // No start: always returns false
        let no_start = Occurrence::new(dir(), &comp, None, None, false);
        assert!(!no_start.is_all_day_on(mid));
    }

    /// Verifies all three branches of `overlaps`: has-start, end-only, and neither.
    #[test]
    fn overlaps_variants() {
        let ev = CalEvent::new("uid-overlap");
        let comp = CalComponent::Event(ev);

        // Branch 1: occurrence has a start — delegates to util::date_ranges_overlap
        let occ_start = utc(2024, 11, 5, 10, 0, 0);
        let occ_end = utc(2024, 11, 5, 11, 0, 0);
        let occ = Occurrence::new(dir(), &comp, Some(occ_start), Some(occ_end), false);

        // overlapping window
        assert!(occ.overlaps(utc(2024, 11, 5, 9, 0, 0), utc(2024, 11, 5, 10, 30, 0)));
        // non-overlapping window (entirely before)
        assert!(!occ.overlaps(utc(2024, 11, 5, 7, 0, 0), utc(2024, 11, 5, 9, 0, 0)));

        // Branch 2: no start but has end (EndOrDue-only occurrence)
        let end_dt = utc(2024, 11, 6, 15, 0, 0);
        let occ_end_only =
            Occurrence::new_single(dir(), &comp, CompDateType::EndOrDue, end_dt, false);
        // end falls inside window
        assert!(occ_end_only.overlaps(utc(2024, 11, 6, 14, 0, 0), utc(2024, 11, 6, 16, 0, 0)));
        // end equals window start (oend >= start is true, oend < end is true) → overlaps
        assert!(occ_end_only.overlaps(utc(2024, 11, 6, 15, 0, 0), utc(2024, 11, 6, 16, 0, 0)));
        // end equals window end (oend < end is false) → no overlap
        assert!(!occ_end_only.overlaps(utc(2024, 11, 6, 15, 0, 0), utc(2024, 11, 6, 15, 0, 0)));
        // end falls outside window
        assert!(!occ_end_only.overlaps(utc(2024, 11, 6, 16, 0, 0), utc(2024, 11, 6, 17, 0, 0)));

        // Branch 3: neither start nor end — always false
        let occ_none = Occurrence::new(dir(), &comp, None, None, false);
        assert!(!occ_none.overlaps(utc(2024, 11, 5, 9, 0, 0), utc(2024, 11, 5, 11, 0, 0)));
    }

    /// Verifies `occurrence_end` falls back to start + time_duration when `end` is None.
    #[test]
    fn occurrence_end_via_duration() {
        let start_dt = utc(2024, 12, 1, 9, 0, 0);
        let mut ev = CalEvent::new("uid-dur");
        ev.set_start(Some(start_dt.into()));
        // Give it a 2-hour duration but no explicit end
        let mut lr = LineReader::new("".as_bytes());
        ev.parse_prop(&mut lr, Property::new("DURATION", vec![], "PT2H"))
            .unwrap();
        let comp = CalComponent::Event(ev);

        let occ = Occurrence::new(dir(), &comp, Some(start_dt), None, false);
        // end must be derived from start + duration
        let expected_end = start_dt + Duration::hours(2);
        assert_eq!(occ.occurrence_end(), Some(expected_end));
    }

    /// Verifies `event_status` and `is_cancelled` for the Event component type.
    #[test]
    fn event_status_and_is_cancelled_event() {
        // Cancelled event
        let mut ev_cancelled = CalEvent::new("ev-cancelled");
        ev_cancelled.set_status(Some(CalEventStatus::Cancelled));
        let comp_cancelled = CalComponent::Event(ev_cancelled);
        let start = utc(2025, 1, 10, 10, 0, 0);
        let occ = Occurrence::new(dir(), &comp_cancelled, Some(start), None, false);

        assert_eq!(occ.event_status(), Some(CalEventStatus::Cancelled));
        assert!(occ.is_cancelled());

        // Non-cancelled event (no status → defaults to Tentative in is_cancelled check)
        let ev_normal = CalEvent::new("ev-normal");
        let comp_normal = CalComponent::Event(ev_normal);
        let occ_normal = Occurrence::new(dir(), &comp_normal, Some(start), None, false);

        assert_eq!(occ_normal.event_status(), None);
        assert!(!occ_normal.is_cancelled());

        // Confirmed event
        let mut ev_confirmed = CalEvent::new("ev-confirmed");
        ev_confirmed.set_status(Some(CalEventStatus::Confirmed));
        let comp_confirmed = CalComponent::Event(ev_confirmed);
        let occ_confirmed = Occurrence::new(dir(), &comp_confirmed, Some(start), None, false);

        assert_eq!(
            occ_confirmed.event_status(),
            Some(CalEventStatus::Confirmed)
        );
        assert!(!occ_confirmed.is_cancelled());
    }

    /// Verifies `todo_status`, `todo_percent`, `todo_completed`, and `is_cancelled` for TODOs.
    #[test]
    fn todo_status_percent_completed_and_is_cancelled_todo() {
        let completed_date = CalDate::DateTime(CalDateTime::Utc(
            Utc.with_ymd_and_hms(2025, 2, 1, 12, 0, 0).unwrap(),
        ));
        let mut td = CalTodo::new("todo-full");
        td.set_status(Some(CalTodoStatus::Cancelled));
        td.set_percent(Some(75));
        td.set_completed(Some(completed_date.clone()));
        let comp = CalComponent::Todo(td);
        let start = utc(2025, 2, 1, 9, 0, 0);
        let occ = Occurrence::new(dir(), &comp, Some(start), None, false);

        assert_eq!(occ.todo_status(), Some(CalTodoStatus::Cancelled));
        assert_eq!(occ.todo_percent(), Some(75));
        assert!(occ.todo_completed().is_some());
        assert!(occ.is_cancelled());

        // Non-cancelled TODO (InProcess)
        let mut td_in_process = CalTodo::new("todo-in-process");
        td_in_process.set_status(Some(CalTodoStatus::InProcess));
        let comp_ip = CalComponent::Todo(td_in_process);
        let occ_ip = Occurrence::new(dir(), &comp_ip, Some(start), None, false);
        assert!(!occ_ip.is_cancelled());

        // No status: is_cancelled uses InProcess as default, which is not cancelled
        let td_no_status = CalTodo::new("todo-no-status");
        let comp_ns = CalComponent::Todo(td_no_status);
        let occ_ns = Occurrence::new(dir(), &comp_ns, Some(start), None, false);
        assert_eq!(occ_ns.todo_status(), None);
        assert!(!occ_ns.is_cancelled());
    }

    /// Verifies the `ctype_method!` macro: the overwrite branch returns its own non-None value.
    #[test]
    fn ctype_method_macro_with_overwrite() {
        // Base: status is None; overwrite: status is Cancelled.
        let ev_base = CalEvent::new("ev-macro");
        let comp_base = CalComponent::Event(ev_base);

        let mut ev_overwrite = CalEvent::new("ev-macro");
        ev_overwrite.set_status(Some(CalEventStatus::Cancelled));
        let comp_overwrite = CalComponent::Event(ev_overwrite);

        let start = utc(2025, 3, 1, 10, 0, 0);
        let mut occ = Occurrence::new(dir(), &comp_base, Some(start), None, false);
        occ.set_overwrite(&comp_overwrite);

        // The overwrite's Cancelled status must win
        assert_eq!(occ.event_status(), Some(CalEventStatus::Cancelled));
        assert!(occ.is_cancelled());

        // Also verify overwrite where base has a status but overwrite has None:
        // the base value should be returned (ctype_method! _ arm).
        let mut ev_base2 = CalEvent::new("ev-macro2");
        ev_base2.set_status(Some(CalEventStatus::Confirmed));
        let comp_base2 = CalComponent::Event(ev_base2);

        let ev_overwrite2_no_status = CalEvent::new("ev-macro2");
        let comp_overwrite2 = CalComponent::Event(ev_overwrite2_no_status);

        let mut occ2 = Occurrence::new(dir(), &comp_base2, Some(start), None, false);
        occ2.set_overwrite(&comp_overwrite2);
        // overwrite has no status → falls back to base's Confirmed
        assert_eq!(occ2.event_status(), Some(CalEventStatus::Confirmed));
    }

    /// Verifies all zero-coverage `EventLike` trait method implementations on `Occurrence`.
    ///
    /// Trivial getter/setter methods are exercised together here. Both the base-only path and the
    /// overwrite-wins path (via `occ_or_base_opt!`) are tested where applicable.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn eventlike_trait_getters_base_and_overwrite() {
        let tz = &chrono_tz::Europe::Berlin;
        let start = tz.with_ymd_and_hms(2025, 4, 1, 9, 0, 0).unwrap();
        let end = tz.with_ymd_and_hms(2025, 4, 1, 10, 0, 0).unwrap();

        // Build a rich base event
        let org = CalOrganizer::new_named("Alice", "alice@example.com");
        let att = CalAttendee::new("mailto:bob@example.com".to_string());
        let alarm = CalAlarm::new(
            CalAction::Display,
            CalTrigger::Relative {
                related: CalRelated::Start,
                duration: Duration::minutes(-10).into(),
            },
        );
        let rrule: CalRRule = "FREQ=WEEKLY;COUNT=3".parse().unwrap();
        let rid = CalDate::DateTime(CalDateTime::Utc(
            Utc.with_ymd_and_hms(2025, 4, 1, 9, 0, 0).unwrap(),
        ));

        let mut ev_base = CalEvent::new("ev-trait");
        ev_base.set_start(Some(start.into()));
        ev_base.set_end(Some(end.into()));
        ev_base.set_summary(Some("Base Summary".to_string()));
        ev_base.set_description(Some("Base Description".to_string()));
        ev_base.set_location(Some("Base Location".to_string()));
        let mut lr = LineReader::new("".as_bytes());
        ev_base
            .parse_prop(&mut lr, Property::new("CATEGORIES", vec![], "cat1"))
            .unwrap();
        ev_base.set_organizer(Some(org.clone()));
        ev_base.set_attendees(Some(vec![att.clone()]));
        ev_base.set_alarms(Some(vec![alarm.clone()]));
        ev_base.set_rrule(Some(rrule.clone()));
        ev_base.set_rid(Some(rid.clone()));
        ev_base.set_priority(Some(3));
        ev_base.toggle_exclude(CalDate::DateTime(CalDateTime::Utc(
            Utc.with_ymd_and_hms(2025, 4, 1, 9, 0, 0).unwrap(),
        )));

        let comp_base = CalComponent::Event(ev_base);
        let occ = Occurrence::new(dir(), &comp_base, Some(start), Some(end), false);

        // EventLike::ctype (the impl on Occurrence, distinct from Occurrence::ctype)
        assert_eq!(EventLike::ctype(&occ), CalCompType::Event);
        // stamp: always present
        let _ = occ.stamp();
        // created / last_modified: present because EventLikeComponent::new sets them
        assert!(occ.created().is_some());
        assert!(occ.last_modified().is_some());
        // start / end_or_due
        assert!(occ.start().is_some());
        assert!(occ.end_or_due().is_some());
        // description / location / categories
        assert_eq!(occ.description(), Some(&"Base Description".to_string()));
        assert_eq!(occ.location(), Some(&"Base Location".to_string()));
        assert_eq!(occ.categories(), Some(["cat1".to_string()].as_ref()));
        // organizer / attendees
        assert!(occ.organizer().is_some());
        assert!(occ.attendees().is_some());
        // exdates: always comes from base regardless of overwrite
        assert_eq!(occ.exdates().len(), 1);
        // alarms / rrule / rid / priority
        assert!(occ.alarms().is_some());
        assert!(occ.rrule().is_some());
        assert!(occ.rid().is_some());
        assert_eq!(occ.priority(), Some(3));

        // occ_or_base_opt! overwrite-wins path: overwrite provides a non-None summary
        let mut ev_overwrite = CalEvent::new("ev-trait");
        ev_overwrite.set_summary(Some("Overwrite Summary".to_string()));
        let comp_overwrite = CalComponent::Event(ev_overwrite);

        let mut occ_ow = Occurrence::new(dir(), &comp_base, Some(start), Some(end), false);
        occ_ow.set_overwrite(&comp_overwrite);

        assert_eq!(occ_ow.summary(), Some(&"Overwrite Summary".to_string()));

        // occ_or_base! (non-opt) path for uid: overwrite's uid is always returned when present
        assert_eq!(occ_ow.uid(), "ev-trait");
    }

    /// Verifies `PropertyProducer::to_props` on `Occurrence` returns an empty vec.
    #[test]
    fn to_props_returns_empty() {
        let ev = CalEvent::new("uid-props");
        let comp = CalComponent::Event(ev);
        let start = utc(2025, 5, 1, 9, 0, 0);
        let occ = Occurrence::new(dir(), &comp, Some(start), None, false);
        assert!(occ.to_props().is_empty());
    }

    /// Verifies all zero-coverage branches in the `time_duration` override on `Occurrence`.
    #[test]
    fn time_duration_overwrite_branches() {
        let start = utc(2025, 6, 1, 9, 0, 0);
        let end = utc(2025, 6, 1, 11, 0, 0); // 2 hours

        // Branch: explicit CalDuration set on overwrite
        let mut ev_dur = CalEvent::new("ev-dur");
        ev_dur.set_start(Some(start.into()));
        let mut lr = LineReader::new("".as_bytes());
        ev_dur
            .parse_prop(&mut lr, Property::new("DURATION", vec![], "PT3H"))
            .unwrap();
        let comp_dur = CalComponent::Event(ev_dur);
        let mut occ_dur = Occurrence::new(dir(), &comp_dur, Some(start), None, false);
        // Attach an overwrite that also has a duration — the overwrite's duration wins via
        // occ_or_base_opt! inside duration(), which then returns early in time_duration().
        let mut ev_ow_dur = CalEvent::new("ev-dur");
        let mut lr2 = LineReader::new("".as_bytes());
        ev_ow_dur
            .parse_prop(&mut lr2, Property::new("DURATION", vec![], "PT5H"))
            .unwrap();
        let comp_ow_dur = CalComponent::Event(ev_ow_dur);
        occ_dur.set_overwrite(&comp_ow_dur);
        assert_eq!(occ_dur.time_duration(), Some(Duration::hours(5)));

        // Branch: overwrite has both start and end (overwrite-derived duration)
        let ostart = utc(2025, 6, 2, 8, 0, 0);
        let oend = utc(2025, 6, 2, 10, 0, 0); // 2 hours
        let mut ev_base = CalEvent::new("ev-both");
        ev_base.set_start(Some(start.into()));
        ev_base.set_end(Some(end.into()));
        let comp_base = CalComponent::Event(ev_base);

        let mut ev_ow = CalEvent::new("ev-both");
        ev_ow.set_start(Some(ostart.into()));
        ev_ow.set_end(Some(oend.into()));
        let comp_ow = CalComponent::Event(ev_ow);

        let mut occ_both = Occurrence::new(dir(), &comp_base, Some(ostart), Some(oend), false);
        occ_both.set_overwrite(&comp_ow);
        assert_eq!(occ_both.time_duration(), Some(Duration::hours(2)));

        // Branch: base has both start and end, overwrite has neither fully overridden
        // → falls back to base.time_duration()
        let mut ev_ow_partial = CalEvent::new("ev-both");
        ev_ow_partial.set_start(Some(ostart.into())); // only start overwritten, no end
        let comp_ow_partial = CalComponent::Event(ev_ow_partial);

        let mut occ_partial = Occurrence::new(dir(), &comp_base, Some(ostart), None, false);
        occ_partial.set_overwrite(&comp_ow_partial);
        // base has start + end → 2 hours
        assert_eq!(occ_partial.time_duration(), Some(Duration::hours(2)));

        // Branch: overwrite has no start/end and base has no start/end → None.
        // We give the occurrence a start so that set_overwrite can call tz().unwrap() safely.
        let ev_no_start = CalEvent::new("ev-nostart");
        let comp_no_start = CalComponent::Event(ev_no_start);
        let ev_ow_no_start = CalEvent::new("ev-nostart"); // no start, no end on overwrite
        let comp_ow_no_start = CalComponent::Event(ev_ow_no_start);

        let mut occ_no_start = Occurrence::new(dir(), &comp_no_start, Some(start), None, false);
        occ_no_start.set_overwrite(&comp_ow_no_start);
        assert_eq!(occ_no_start.time_duration(), None);

        // Branch: no overwrite, all-day base where start is not CalDate::Date
        // (the normalization branch in time_duration that converts DateTime start to Date)
        let date = NaiveDate::from_ymd_opt(2025, 6, 3).unwrap();
        let mut ev_allday_mixed = CalEvent::new("ev-allday-mixed");
        // start as DATE (all-day), end as DATE one day later
        ev_allday_mixed.set_start(Some(CalDate::Date(date, CalCompType::Event.into())));
        ev_allday_mixed.set_end(Some(CalDate::Date(
            date.succ_opt().unwrap(),
            CalCompType::Event.into(),
        )));
        let comp_allday = CalComponent::Event(ev_allday_mixed);
        let start_allday = UTC.with_ymd_and_hms(2025, 6, 3, 0, 0, 0).unwrap();
        let occ_allday = Occurrence::new(dir(), &comp_allday, Some(start_allday), None, false);
        // The end is stored as DATE(2025-06-04, Exclusive), which as_end_with_tz resolves to
        // 2025-06-03T23:59:59. Subtracting the start (00:00:00) gives 86399 seconds.
        assert_eq!(occ_allday.time_duration(), Some(Duration::seconds(86399)));
    }

    /// Verifies `AlarmOccurrence::new`, `occurrence`, `alarm`, and `alarm_date`.
    #[test]
    fn alarm_occurrence_getters_and_alarm_date() {
        let start = utc(2025, 7, 15, 10, 0, 0);
        let ev = timed_event("uid-alarm-occ", start);
        let comp = CalComponent::Event(ev);
        let occ = Occurrence::new(dir(), &comp, Some(start), None, false);

        let alarm = CalAlarm::new(
            CalAction::Display,
            CalTrigger::Relative {
                related: CalRelated::Start,
                duration: Duration::minutes(-15).into(),
            },
        );

        let alarm_occ = AlarmOccurrence::new(occ, alarm.clone());

        assert_eq!(alarm_occ.occurrence().uid(), "uid-alarm-occ");
        assert_eq!(alarm_occ.alarm().action(), CalAction::Display);

        // alarm fires 15 minutes before start
        let expected = start - Duration::minutes(15);
        assert_eq!(alarm_occ.alarm_date(), Some(expected));
    }
}
