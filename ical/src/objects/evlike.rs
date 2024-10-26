use crate::objects::{CalAttendee, CalDate, CalRRule};

pub trait EventLike {
    fn uid(&self) -> &String;
    fn created(&self) -> &CalDate;
    fn last_modified(&self) -> &CalDate;

    fn start(&self) -> Option<&CalDate>;
    fn start_or_created(&self) -> &CalDate {
        self.start().unwrap_or(self.created())
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
    fn attendees(&self) -> &[CalAttendee];

    fn rrule(&self) -> Option<&CalRRule>;
    fn rid(&self) -> Option<&CalDate>;
}
