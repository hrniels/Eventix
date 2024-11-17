use anyhow::anyhow;
use std::io::{BufRead, Write};
use std::str::FromStr;

use crate::objects::{CalComponent, CalEvent, CalTodo, EventLike};
use crate::parser::{LineReader, LineWriter, Property, PropertyConsumer, PropertyProducer};

#[derive(Default, Debug, Eq, PartialEq)]
pub struct Calendar {
    comps: Vec<CalComponent>,
    props: Vec<Property>,
    other: Vec<Other>,
}

impl Calendar {
    pub fn properties(&self) -> &[Property] {
        &self.props
    }

    pub fn components(&self) -> &[CalComponent] {
        &self.comps
    }

    pub fn components_mut(&mut self) -> &mut [CalComponent] {
        &mut self.comps
    }

    pub fn add(&mut self, comp: CalComponent) {
        self.comps.push(comp);
    }

    pub fn delete_components<N: AsRef<str>>(&mut self, uid: N) {
        self.comps.retain(|c| c.uid() != uid.as_ref());
    }

    pub fn write<W: Write>(&self, writer: W) -> Result<(), anyhow::Error> {
        let mut wr = LineWriter::new(writer);
        wr.write_line("BEGIN:VCALENDAR")?;
        for p in self.to_props() {
            wr.write_line(p.to_string())?;
        }
        wr.write_line("END:VCALENDAR")
    }
}

impl PropertyProducer for Calendar {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![];
        props.extend(self.props.iter().cloned());
        for other in &self.other {
            props.extend(other.to_props().into_iter());
        }
        for comp in &self.comps {
            props.extend(comp.to_props().into_iter());
        }
        props
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
                    let other = Other::from_lines(lines, prop)?;
                    cal.other.push(other);
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

#[derive(Debug, Eq, PartialEq)]
pub struct Other {
    name: String,
    props: Vec<Property>,
}

impl Other {
    pub fn new<N: ToString>(name: N) -> Self {
        Self {
            name: name.to_string(),
            props: Vec::new(),
        }
    }

    pub fn add(&mut self, prop: Property) {
        self.props.push(prop);
    }
}

impl PropertyProducer for Other {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![Property::new("BEGIN", vec![], self.name.clone())];
        props.extend(self.props.iter().cloned());
        props.push(Property::new("END", vec![], self.name.clone()));
        props
    }
}

impl PropertyConsumer for Other {
    fn from_lines<R: BufRead>(
        lines: &mut LineReader<R>,
        prop: Property,
    ) -> Result<Self, anyhow::Error>
    where
        Self: Sized,
    {
        let mut other = Other::new(prop.take_value());
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
                    other.add(prop);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufWriter;

    use chrono::NaiveDate;

    use crate::{
        objects::{CalComponent, CalDate, CalDateTime, Calendar, EventLike},
        parser::Property,
    };

    #[test]
    fn basics() {
        let ical = "BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VTODO
CREATED:20241010T101222Z
LAST-MODIFIED:20241010T101222Z
DTSTAMP:20241024T090000Z
DTSTART;TZID=\"My:TZ\":20241024T090000
SUMMARY:foo bar
 test with\\n
  multiple\\;\\,
  lines
DESCRIPTION:test!
CATEGORIES:A,B,MYCAT\r
ATTENDEE;PARTSTAT=ACCEPTED;CN=\"My,Name\":my@name.org
ATTENDEE;CN=test:test@example.com\r
PRIORITY:7\r
RID:20221110T111111Z
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
            Some(&"foo bartest with\n multiple;, lines".to_string())
        );

        let mut res = Vec::new();
        let writer = BufWriter::new(&mut res);
        ical.write(writer).unwrap();
        let res = String::from_utf8(res).unwrap();
        assert_eq!(
            res,
            "BEGIN:VCALENDAR\r
VERSION:2.0\r
BEGIN:VTODO\r
UID:1234-5678\r
CREATED:20241010T101222Z\r
LAST-MODIFIED:20241010T101222Z\r
DTSTAMP:20241024T090000Z\r
DTSTART;TZID=\"My:TZ\":20241024T090000\r
SUMMARY:foo bartest with\\n multiple\\;\\, lines\r
DESCRIPTION:test!\r
CATEGORIES:A,B,MYCAT\r
ATTENDEE;PARTSTAT=ACCEPTED;CN=\"My,Name\":my@name.org\r
ATTENDEE;CN=test:test@example.com\r
PRIORITY:7\r
RID:20221110T111111Z\r
TEST;FOO=bar;A=B:\"value\"\r
END:VTODO\r
END:VCALENDAR\r
"
        );
    }

    #[test]
    fn basic_tostr() {}
}
