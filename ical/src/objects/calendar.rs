use anyhow::anyhow;
use std::io::BufRead;
use std::str::FromStr;

use crate::objects::{CalEvent, CalTodo};
use crate::parser::{LineReader, Property, PropertyConsumer};

#[derive(Debug)]
pub enum CalComponent {
    Event(CalEvent),
    Todo(CalTodo),
    Other(Other),
}

impl CalComponent {
    pub fn as_event(&self) -> Option<&CalEvent> {
        match self {
            Self::Event(ev) => Some(ev),
            _ => None,
        }
    }

    pub fn as_todo(&self) -> Option<&CalTodo> {
        match self {
            Self::Todo(todo) => Some(todo),
            _ => None,
        }
    }
}

#[derive(Default, Debug)]
pub struct Calendar {
    comps: Vec<CalComponent>,
    props: Vec<Property>,
}

impl Calendar {
    pub fn properties(&self) -> &[Property] {
        &self.props
    }

    pub fn components(&self) -> &[CalComponent] {
        &self.comps
    }

    pub fn add(&mut self, comp: CalComponent) {
        self.comps.push(comp);
    }
}

impl PropertyConsumer for Calendar {
    fn from_lines<R: BufRead>(
        lines: &mut LineReader<R>,
        _prop: Property,
    ) -> Result<Self, anyhow::Error>
    where
        Self: Sized,
    {
        let mut cal = Self::default();
        loop {
            let Some(line) = lines.next() else {
                break Ok(cal);
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                "BEGIN" if prop.value() == "VTODO" => {
                    let todo = CalComponent::Todo(CalTodo::from_lines(lines, prop)?);
                    cal.comps.push(todo);
                }
                "BEGIN" if prop.value() == "VEVENT" => {
                    let event = CalComponent::Event(CalEvent::from_lines(lines, prop)?);
                    cal.comps.push(event);
                }
                "BEGIN" => {
                    let other = CalComponent::Other(Other::from_lines(lines, prop)?);
                    cal.comps.push(other);
                }
                "END" => {
                    if prop.value() != "VCALENDAR" {
                        return Err(anyhow!("Unexpected END:{}", prop.value()));
                    }
                    break Ok(cal);
                }
                _ => {
                    cal.props.push(prop);
                }
            }
        }
    }
}

impl FromStr for Calendar {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut lines = LineReader::new(s.as_bytes());
        let Some(line) = lines.next() else {
            return Err(anyhow!("Unexpected EOF"));
        };

        let prop = line.parse::<Property>()?;
        match prop.name().as_str() {
            "BEGIN" if prop.value() == "VCALENDAR" => {
                let cal = Calendar::from_lines(&mut lines, prop)?;
                Ok(cal)
            }
            _ => Err(anyhow!("Unexpected property: {:?}", prop)),
        }
    }
}

#[derive(Debug)]
pub struct Other {
    name: String,
    props: Vec<Property>,
}

impl PropertyConsumer for Other {
    fn from_lines<R: BufRead>(
        lines: &mut LineReader<R>,
        prop: Property,
    ) -> Result<Self, anyhow::Error>
    where
        Self: Sized,
    {
        let mut other = Self {
            name: prop.take_value(),
            props: Vec::new(),
        };
        loop {
            let Some(line) = lines.next() else {
                break Err(anyhow!("Unexpected EOF"));
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                "END" if prop.value() == &other.name => {
                    break Ok(other);
                }
                _ => {
                    other.props.push(prop);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use crate::{
        objects::{CalComponent, CalDate, CalDateTime, Calendar},
        parser::Property,
    };

    #[test]
    fn basics() {
        let ical = "BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VTODO
DTSTART;TZID=\"My:TZ\":20241024T090000
SUMMARY:foo bar
 test with
  multiple
  lines
UID:1234-5678
TEST;FOO=bar;A=B:\"value\"
END:VTODO
END:VCALENDAR";

        let ical = ical.parse::<Calendar>().unwrap();
        assert_eq!(ical.props.len(), 1);
        assert_eq!(ical.props[0], Property::new("VERSION", vec![], "2.0"));
        assert_eq!(ical.comps.len(), 1);
        assert!(matches!(ical.comps[0], CalComponent::Todo(_)));
        let CalComponent::Todo(ref todo) = ical.comps[0] else {
            panic!("Expecting TODO");
        };
        assert_eq!(todo.uid().as_str(), "1234-5678");
        assert_eq!(
            todo.start(),
            Some(&CalDate::DateTime(CalDateTime::Timezone(
                NaiveDate::from_ymd_opt(2024, 10, 24)
                    .unwrap()
                    .and_hms_opt(9, 0, 0)
                    .unwrap(),
                "My:TZ".to_string()
            )))
        );
        assert_eq!(
            todo.summary(),
            Some(&"foo bartest with multiple lines".to_string())
        );
    }
}
