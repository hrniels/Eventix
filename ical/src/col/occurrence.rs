use std::sync::Arc;

use chrono::{DateTime, Duration, NaiveDate};
use chrono_tz::Tz;

use crate::objects::{
    CalAlarm, CalAttendee, CalCompType, CalComponent, CalDate, CalEventStatus, CalOrganizer,
    CalRRule, CalTodoStatus, EventLike,
};
use crate::parser::{Property, PropertyProducer};
use crate::util;

macro_rules! ctype_method {
    ($self:expr, $ctype:tt, $method:tt) => {
        match $self.occ {
            Some(c) if c.$ctype().and_then(|td| td.$method()).is_some() => {
                c.$ctype().and_then(|td| td.$method())
            }
            _ => $self.base.$ctype().and_then(|td| td.$method()),
        }
    };
}

#[derive(Debug, Clone)]
pub struct Occurrence<'c> {
    source: Arc<String>,
    start: DateTime<Tz>,
    base: &'c CalComponent,
    occ: Option<&'c CalComponent>,
}

impl<'c> Occurrence<'c> {
    pub fn new(source: Arc<String>, base: &'c CalComponent, start: DateTime<Tz>) -> Self {
        Self {
            source,
            start,
            base,
            occ: None,
        }
    }

    pub fn source(&self) -> &Arc<String> {
        &self.source
    }

    pub fn ctype(&self) -> CalCompType {
        self.base.ctype()
    }

    pub fn is_overwritten(&self) -> bool {
        self.occ.is_some()
    }

    pub fn event_status(&self) -> Option<CalEventStatus> {
        ctype_method!(self, as_event, status)
    }

    pub fn todo_status(&self) -> Option<CalTodoStatus> {
        ctype_method!(self, as_todo, status)
    }

    pub fn todo_percent(&self) -> Option<u8> {
        ctype_method!(self, as_todo, percent)
    }

    pub fn todo_completed(&self) -> Option<&CalDate> {
        ctype_method!(self, as_todo, completed)
    }

    pub fn set_occurrence(&mut self, occ: &'c CalComponent) {
        self.occ = Some(occ);
        if let Some(ostart) = occ.start() {
            self.start = ostart.as_start_with_tz(&self.start.timezone());
        }
    }

    pub fn occurrence_start(&self) -> DateTime<Tz> {
        self.start
    }

    pub fn occurrence_startdate(&self) -> CalDate {
        if self.is_all_day() {
            CalDate::Date(self.start.date_naive())
        } else {
            self.start.into()
        }
    }

    pub fn occurrence_end(&self) -> Option<DateTime<Tz>> {
        self.duration().map(|d| self.start + d)
    }

    pub fn occurrence_enddate(&self) -> Option<CalDate> {
        self.occurrence_end().and_then(|e| {
            if self.is_all_day() {
                Some(CalDate::Date(e.date_naive().succ_opt()?))
            } else {
                Some(e.into())
            }
        })
    }

    pub fn occurrence_starts_on(&self, date: NaiveDate) -> bool {
        self.occurrence_start().date_naive() == date
    }

    pub fn occurrence_ends_on(&self, date: NaiveDate) -> bool {
        self.occurrence_end()
            .map(|end| end.date_naive() == date)
            .unwrap_or(false)
    }

    pub fn is_all_day_on(&self, date: NaiveDate) -> bool {
        let end = self
            .occurrence_end()
            .map(|e| e.date_naive())
            .unwrap_or(date);
        date > self.occurrence_start().date_naive() && date < end
    }

    pub fn duration(&self) -> Option<Duration> {
        self.base.duration(&self.start.timezone())
    }

    pub fn alarm_date(&self) -> Option<DateTime<Tz>> {
        self.alarms()
            .first()
            .and_then(|a| a.trigger_date(Some(self.occurrence_start()), self.occurrence_end()))
    }

    pub fn overlaps(&self, start: DateTime<Tz>, end: DateTime<Tz>) -> bool {
        util::date_ranges_overlap(
            self.start,
            self.occurrence_end().unwrap_or(self.start),
            start,
            end,
        )
    }
}

macro_rules! occ_or_base {
    ($self:tt, $method:tt) => {
        match $self.occ {
            Some(occ) => occ.$method(),
            _ => $self.base.$method(),
        }
    };
}

macro_rules! occ_or_base_opt {
    ($self:tt, $method:tt) => {
        match $self.occ {
            Some(occ) if occ.$method().is_some() => occ.$method(),
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

    fn alarms(&self) -> &[CalAlarm] {
        occ_or_base!(self, alarms)
    }

    fn rrule(&self) -> Option<&CalRRule> {
        occ_or_base_opt!(self, rrule)
    }

    fn rid(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, rid)
    }
}
