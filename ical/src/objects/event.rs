use anyhow::anyhow;
use std::io::BufRead;
use std::ops::{Deref, DerefMut};

use crate::objects::{CalDate, CalEventStatus, Other};
use crate::parser::{LineReader, Property, PropertyConsumer, PropertyProducer};

use super::component::EventLikeComponent;

#[derive(Default, Debug)]
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

    pub fn set_end(&mut self, end: CalDate) {
        self.end = Some(end);
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
        vec![]
    }
}

impl PropertyConsumer for CalEvent {
    fn from_lines<R: BufRead>(
        lines: &mut LineReader<R>,
        _prop: Property,
    ) -> Result<Self, anyhow::Error>
    where
        Self: Sized,
    {
        let mut comp = Self::default();
        loop {
            let Some(line) = lines.next() else {
                break Err(anyhow!("Unexpected EOF"));
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
