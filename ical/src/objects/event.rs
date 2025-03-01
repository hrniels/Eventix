use std::io::BufRead;
use std::ops::{Deref, DerefMut};

use crate::objects::{CalDate, CalEventStatus};
use crate::parser::{LineReader, ParseError, Property, PropertyConsumer, PropertyProducer};

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
            inner: EventLikeComponent::new_empty(),
            status: None,
            end: None,
        }
    }

    /// Creates a new event with given uid.
    pub fn new<T: ToString>(uid: T) -> Self {
        let mut new = Self::new_empty();
        new.inner = EventLikeComponent::new(uid);
        new
    }

    /// Returns the status of the event.
    pub fn status(&self) -> Option<CalEventStatus> {
        self.status
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
