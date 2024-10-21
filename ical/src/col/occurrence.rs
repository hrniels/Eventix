use chrono::{DateTime, Duration};
use chrono_tz::Tz;

use crate::col::Id;
use crate::objects::CalComponent;

#[derive(Debug)]
pub struct Occurrence<'c> {
    source: Id,
    comp: &'c CalComponent,
    start: DateTime<Tz>,
}

impl<'c> Occurrence<'c> {
    pub fn new(source: Id, comp: &'c CalComponent, start: DateTime<Tz>) -> Self {
        Self {
            source,
            comp,
            start,
        }
    }

    pub fn source(&self) -> Id {
        self.source
    }

    pub fn component(&self) -> &CalComponent {
        self.comp
    }

    pub fn set_component(&mut self, comp: &'c CalComponent) {
        self.comp = comp;
    }

    pub fn start(&self) -> DateTime<Tz> {
        self.start
    }

    pub fn set_start(&mut self, start: DateTime<Tz>) {
        self.start = start;
    }

    pub fn end(&self) -> Option<DateTime<Tz>> {
        self.duration().map(|d| self.start + d)
    }

    pub fn duration(&self) -> Option<Duration> {
        match (self.comp.start(), self.comp.end_or_due()) {
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

        if let Some(occ_end) = self.end() {
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
