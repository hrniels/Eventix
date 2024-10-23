use chrono::{DateTime, Duration};
use chrono_tz::Tz;

use crate::col::Id;
use crate::objects::{
    CalComponent, CalDate, CalEvent, CalEventStatus, CalRRule, CalTodo, EventLike,
};

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

    pub fn event_status(&self) -> Option<CalEventStatus> {
        match self.occ {
            Some(c) => c.as_event().and_then(|ev| ev.status()),
            None => self.base.as_event().and_then(|ev| ev.status()),
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
        match (self.start(), self.end_or_due()) {
            (Some(start), Some(end)) => Some(
                end.as_end_with_tz(&self.start.timezone())
                    - start.as_start_with_tz(&self.start.timezone()),
            ),
            _ => None,
        }
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

    fn rrule(&self) -> Option<&CalRRule> {
        occ_or_base_opt!(self, rrule)
    }

    fn rid(&self) -> Option<&CalDate> {
        occ_or_base_opt!(self, rid)
    }
}
