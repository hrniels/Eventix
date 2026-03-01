use std::io::BufRead;
use std::ops::{Deref, DerefMut};

use crate::objects::{CalDate, CalEventStatus};
use crate::parser::{LineReader, ParseError, Property, PropertyConsumer, PropertyProducer};

use super::CalCompType;
use super::component::EventLikeComponent;

/// Represents an iCalendar event.
///
/// Each event has a unique id (uid) and several optional properties such as a summary, a
/// description, or alarms. An event shares many properties with
/// [`CalTodo`](crate::objects::CalTodo), which are implemented in [`EventLikeComponent`]. In
/// contrast to TODOs, events have a [`CalEventStatus`] and an end date instead of a due date.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.6.1>.
#[derive(Debug, Eq, PartialEq)]
pub struct CalEvent {
    pub(crate) inner: EventLikeComponent,
    status: Option<CalEventStatus>,
    end: Option<CalDate>,
}

impl CalEvent {
    fn new_empty() -> Self {
        Self {
            inner: EventLikeComponent::new_empty(CalCompType::Event),
            status: None,
            end: None,
        }
    }

    /// Creates a new event with given uid.
    pub fn new<T: ToString>(uid: T) -> Self {
        let mut new = Self::new_empty();
        new.inner = EventLikeComponent::new(uid, CalCompType::Event);
        new
    }

    /// Returns the status of the event.
    pub fn status(&self) -> Option<CalEventStatus> {
        self.status
    }

    /// Sets the status to given value.
    pub fn set_status(&mut self, status: Option<CalEventStatus>) {
        self.status = status;
    }

    /// Returns the end of the event.
    pub fn end(&self) -> Option<&CalDate> {
        self.end.as_ref()
    }

    /// Sets the event end to given value.
    pub fn set_end(&mut self, end: Option<CalDate>) {
        self.end = end;
    }
}

impl Deref for CalEvent {
    type Target = EventLikeComponent;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for CalEvent {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl PropertyProducer for CalEvent {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![Property::new("BEGIN", vec![], "VEVENT")];
        if let Some(ref dtend) = self.end {
            props.push(dtend.to_prop("DTEND"));
        }
        if let Some(ref status) = self.status {
            props.push(Property::new("STATUS", vec![], status.to_string()));
        }
        props.extend(self.inner.to_props());
        props.push(Property::new("END", vec![], "VEVENT"));
        props
    }
}

impl PropertyConsumer for CalEvent {
    fn from_lines<R: BufRead>(
        lines: &mut LineReader<R>,
        _prop: Property,
    ) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        let mut comp = Self::new_empty();
        loop {
            let Some(line) = lines.next() else {
                break Err(ParseError::UnexpectedEOF);
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                "END" if prop.value() == "VEVENT" => {
                    break Ok(comp);
                }
                "STATUS" => {
                    comp.status = Some(prop.value().parse()?);
                }
                "DTEND" => {
                    comp.end = Some(prop.try_into()?);
                }
                _ => {
                    comp.inner.parse_prop(lines, prop)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::EventLike;
    use crate::parser::{LineReader, Property};

    #[test]
    fn parse_and_to_props_roundtrip() {
        let data = "UID:uid-1\n\
DTSTAMP:20250102T090000Z\n\
DTSTART:20250102T100000Z\n\
DTEND:20250102T120000Z\n\
STATUS:CONFIRMED\n\
SUMMARY:Meeting\n\
END:VEVENT\n";
        let mut lines = LineReader::new(data.as_bytes());
        let begin_prop = "BEGIN:VEVENT".parse::<Property>().unwrap();
        let ev = CalEvent::from_lines(&mut lines, begin_prop).expect("failed to parse VEVENT");

        // basics
        assert_eq!(ev.uid().as_str(), "uid-1");
        assert_eq!(ev.status(), Some(CalEventStatus::Confirmed));

        // end is a datetime in UTC and must match the exact textual representation when printed
        let end = ev.end().expect("end missing").to_string();
        assert_eq!(end, "TU2025-01-02T12:00:00");

        // start and summary were parsed into the inner component
        let start_prop = ev.start().expect("start missing").to_string();
        assert_eq!(start_prop, "TU2025-01-02T10:00:00");
        assert_eq!(ev.summary(), Some(&"Meeting".to_string()));

        // check produced properties are in the exact order expected by to_props
        let props: Vec<String> = ev.to_props().into_iter().map(|p| p.to_string()).collect();
        let expected = vec![
            "BEGIN:VEVENT".to_string(),
            "DTEND:20250102T120000Z".to_string(),
            "STATUS:CONFIRMED".to_string(),
            "UID:uid-1".to_string(),
            "DTSTAMP:20250102T090000Z".to_string(),
            "DTSTART:20250102T100000Z".to_string(),
            "SUMMARY:Meeting".to_string(),
            "END:VEVENT".to_string(),
        ];
        assert_eq!(props, expected);
    }

    #[test]
    fn status_and_end_setters() {
        let mut ev = CalEvent::new("my-uid");
        ev.set_status(Some(CalEventStatus::Tentative));
        assert_eq!(ev.status(), Some(CalEventStatus::Tentative));

        ev.set_status(None);
        assert_eq!(ev.status(), None);

        // set and clear end
        let dtend = "DTEND:20250103T010203Z"
            .parse::<Property>()
            .unwrap()
            .try_into()
            .unwrap();
        ev.set_end(Some(dtend));
        assert!(ev.end().is_some());
        ev.set_end(None);
        assert!(ev.end().is_none());
    }
}
