use anyhow::anyhow;
use std::io::BufRead;

use crate::objects::{CalDate, RecurrenceRule};
use crate::parser::{LineReader, Property, PropertyConsumer};

#[derive(Default, Debug)]
pub struct Event {
    uid: String,
    created: CalDate,
    summary: Option<String>,
    start: Option<CalDate>,
    end: Option<CalDate>,
    rrule: Option<RecurrenceRule>,
    props: Vec<Property>,
}

impl Event {
    pub fn uid(&self) -> &String {
        &self.uid
    }

    pub fn set_uid<T: ToString>(&mut self, uid: T) {
        self.uid = uid.to_string();
    }

    pub fn start(&self) -> Option<&CalDate> {
        self.start.as_ref()
    }

    pub fn set_start(&mut self, start: CalDate) {
        self.start = Some(start);
    }

    pub fn end(&self) -> Option<&CalDate> {
        self.end.as_ref()
    }

    pub fn set_end(&mut self, end: CalDate) {
        self.end = Some(end);
    }

    pub fn rrule(&self) -> Option<&RecurrenceRule> {
        self.rrule.as_ref()
    }

    pub fn summary(&self) -> Option<&String> {
        self.summary.as_ref()
    }
}

impl PropertyConsumer for Event {
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
                "END" if prop.value() == "VEVENT" => {
                    break Ok(comp);
                }
                "UID" => {
                    comp.uid = prop.take_value();
                }
                "CREATED" => {
                    comp.created = prop.try_into()?;
                }
                "SUMMARY" => {
                    comp.summary = Some(prop.take_value());
                }
                "DTSTART" => {
                    comp.start = Some(prop.try_into()?);
                }
                "DTEND" => {
                    comp.end = Some(prop.try_into()?);
                }
                "RRULE" => {
                    comp.rrule = Some(prop.value().parse()?);
                }
                _ => {
                    comp.props.push(prop);
                }
            }
        }
    }
}
