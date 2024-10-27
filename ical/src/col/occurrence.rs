use chrono::{DateTime, Duration};
use chrono_tz::Tz;

use crate::col::Id;
use crate::objects::{
    CalAttendee, CalComponent, CalDate, CalEventStatus, CalRRule, CalTodoStatus, EventLike,
};
use crate::parser::{Property, PropertyProducer};

#[derive(Debug)]
pub struct Occurrence<'c> {
    source: Id,
    start: DateTime<Tz>,
    base: &'c CalComponent,
    occ: Option<&'c CalComponent>,
}

impl<'c> Occurrence<'c> {
    pub fn new(source: Id, base: &'c CalComponent, start: DateTime<Tz>) -> Self {
        Self {
            source,
            start,
            base,
            occ: None,
        }
    }

    pub fn source(&self) -> Id {
        self.source
    }

    pub fn is_event(&self) -> bool {
        self.base.is_event()
    }

    pub fn is_todo(&self) -> bool {
        self.base.is_todo()
    }

    pub fn is_overwritten(&self) -> bool {
        self.occ.is_some()
    }

    pub fn event_status(&self) -> Option<CalEventStatus> {
        match self.occ {
            Some(c) => c.as_event().and_then(|ev| ev.status()),
            None => self.base.as_event().and_then(|ev| ev.status()),
        }
    }

    pub fn todo_status(&self) -> Option<CalTodoStatus> {
        match self.occ {
            Some(c) => c.as_todo().and_then(|td| td.status()),
            None => self.base.as_todo().and_then(|td| td.status()),
        }
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

    pub fn occurrence_end(&self) -> Option<DateTime<Tz>> {
        self.duration().map(|d| self.start + d)
    }

    pub fn duration(&self) -> Option<Duration> {
        let start = self.start_or_created();

        // ensure that we start day-aligned if either start or end is all-day
        let start = if self.is_all_day() && !matches!(start, CalDate::Date(_)) {
            CalDate::Date(start.as_naive_date())
        } else {
            start.clone()
        };

        self.end_or_due().map(|end| {
            end.as_end_with_tz(&self.start.timezone())
                - start.as_start_with_tz(&self.start.timezone())
        })
    }

    pub fn overlaps(&self, start: DateTime<Tz>, end: DateTime<Tz>) -> bool {
        if self.start >= start && self.start <= end {
            return true;
        }

        if let Some(occ_end) = self.occurrence_end() {
            if occ_end >= start && occ_end <= end {
                return true;
            }
            if self.start < start && occ_end > end {
                return true;
            }
        }
        false
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

    fn created(&self) -> &CalDate {
        occ_or_base!(self, created)
    }

    fn last_modified(&self) -> &CalDate {
        occ_or_base!(self, last_modified)
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

    fn categories(&self) -> &[String] {
        occ_or_base!(self, categories)
    }

    fn attendees(&self) -> &[CalAttendee] {
        occ_or_base!(self, attendees)
    }

    fn rrule(&self) -> Option<&CalRRule> {
        occ_or_base_opt!(self, rrule)
    }

    fn rid(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, rid)
    }
}
