use std::io::BufRead;
use std::ops::{Deref, DerefMut};

use crate::objects::{CalDate, CalEventStatus, Other};
use crate::parser::{LineReader, ParseError, Property, PropertyConsumer, PropertyProducer};

use super::component::EventLikeComponent;

#[derive(Default, Debug, Eq, PartialEq)]
pub struct CalEvent {
    pub(crate) inner: EventLikeComponent,
    status: Option<CalEventStatus>,
    end: Option<CalDate>,
    other: Vec<Other>,
}

impl CalEvent {
    pub fn status(&self) -> Option<CalEventStatus> {
        self.status
    }

    pub fn end(&self) -> Option<&CalDate> {
        self.end.as_ref()
    }

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
        for o in &self.other {
            props.extend(o.to_props().into_iter());
        }
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
        let mut comp = Self::default();
        loop {
            let Some(line) = lines.next() else {
                break Err(ParseError::UnexpectedEOF);
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                // TODO properly support alarms
                "BEGIN" => {
                    let other = Other::from_lines(lines, prop)?;
                    comp.other.push(other);
                }
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
                    comp.inner.parse_prop(prop)?;
                }
            }
        }
    }
}
