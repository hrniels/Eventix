use std::sync::Arc;

use chrono::{DateTime, Duration, NaiveDate};
use chrono_tz::Tz;

use crate::objects::{
    CalAlarm, CalAttendee, CalCompType, CalComponent, CalDate, CalEventStatus, CalOrganizer,
    CalRRule, CalTodoStatus, CompDateType, EventLike,
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
    start: Option<DateTime<Tz>>,
    end: Option<DateTime<Tz>>,
    base: &'c CalComponent,
    occ: Option<&'c CalComponent>,
}

impl<'c> Occurrence<'c> {
    pub fn new(
        source: Arc<String>,
        base: &'c CalComponent,
        start: Option<DateTime<Tz>>,
        end: Option<DateTime<Tz>>,
    ) -> Self {
        Self {
            source,
            start,
            end,
            base,
            occ: None,
        }
    }

    pub fn new_single(
        source: Arc<String>,
        base: &'c CalComponent,
        ty: CompDateType,
        date: DateTime<Tz>,
    ) -> Self {
        Self::new(
            source,
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
        )
    }

    pub fn tz(&self) -> Option<Tz> {
        match self.start {
            Some(start) => Some(start.timezone()),
            None => self.end.map(|end| end.timezone()),
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
            self.start = Some(ostart.as_start_with_tz(&self.tz().unwrap()));
        }
    }

    pub fn occurrence_start(&self) -> Option<DateTime<Tz>> {
        self.start
    }

    pub fn occurrence_startdate(&self) -> Option<CalDate> {
        self.start.map(|start| {
            if self.is_all_day() {
                CalDate::Date(start.date_naive())
            } else {
                start.into()
            }
        })
    }

    pub fn occurrence_end(&self) -> Option<DateTime<Tz>> {
        match self.end {
            Some(end) => Some(end),
            None => self.duration().map(|d| self.start.unwrap() + d),
        }
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
        match self.occurrence_start() {
            Some(start) => start.date_naive() == date,
            None => false,
        }
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
        match self.occurrence_start() {
            Some(start) => date > start.date_naive() && date < end,
            None => false,
        }
    }

    pub fn duration(&self) -> Option<Duration> {
        self.tz().and_then(|tz| self.base.duration(&tz))
    }

    pub fn alarm_date(&self) -> Option<DateTime<Tz>> {
        self.alarms()
            .first()
            .and_then(|a| a.trigger_date(self.occurrence_start(), self.occurrence_end()))
    }

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
