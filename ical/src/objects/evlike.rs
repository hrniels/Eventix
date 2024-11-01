use crate::{
    objects::{CalAttendee, CalDate, CalOrganizer, CalRRule},
    parser::PropertyProducer,
};

pub trait EventLike: PropertyProducer {
    fn uid(&self) -> &String;
    fn stamp(&self) -> &CalDate;
    fn created(&self) -> Option<&CalDate>;
    fn last_modified(&self) -> Option<&CalDate>;

    fn start(&self) -> Option<&CalDate>;
    fn start_or_created(&self) -> Option<&CalDate> {
        match self.start() {
            Some(st) => Some(st),
            None => self.created(),
        }
    }
    fn end_or_due(&self) -> Option<&CalDate>;
    fn is_all_day(&self) -> bool {
        matches!(self.start(), Some(CalDate::Date(_)))
            || matches!(self.end_or_due(), Some(CalDate::Date(_)))
    }

    fn summary(&self) -> Option<&String>;
    fn description(&self) -> Option<&String>;
    fn location(&self) -> Option<&String>;
    fn categories(&self) -> &[String];
    fn organizer(&self) -> Option<&CalOrganizer>;
    fn attendees(&self) -> &[CalAttendee];

    fn rrule(&self) -> Option<&CalRRule>;
    fn rid(&self) -> Option<&CalDate>;

    fn is_recurrent(&self) -> bool {
        self.rrule().is_some() || self.rid().is_some()
    }
}

pub trait UpdatableEventLike: EventLike {
    fn set_summary(&mut self, summary: String);
    fn set_last_modified(&mut self, date: CalDate);
    fn set_stamp(&mut self, date: CalDate);
}
