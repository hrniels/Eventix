use std::io::{self, BufRead, Write};
use std::str::FromStr;

use tracing::warn;

use crate::objects::{CalCompType, CalComponent, CalEvent, CalTodo, EventLike};
use crate::parser::{
    LineReader, LineWriter, ParseError, Property, PropertyConsumer, PropertyProducer,
};

/// Represents an iCalendar object.
///
/// Such a calendar consists of one or more [`CalComponent`]s, each being either an event or TODO.
/// Additionally, the calendar itself can have properties such as the version or product id.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.4>.
#[derive(Default, Debug, Eq, PartialEq)]
pub struct Calendar {
    comps: Vec<CalComponent>,
    timezones: Vec<CalTimeZone>,
    props: Vec<Property>,
    unknown: Vec<Unknown>,
}

impl Calendar {
    /// Returns a slice of the calendar properties.
    pub fn properties(&self) -> &[Property] {
        &self.props
    }

    /// Returns a slice of the timezone components.
    pub fn timezones(&self) -> &[CalTimeZone] {
        &self.timezones
    }

    /// Adds the given timezone to the calendar.
    pub fn add_timezone(&mut self, tz: CalTimeZone) {
        self.timezones.push(tz);
    }

    /// Returns a slice of the calendar components.
    pub fn components(&self) -> &[CalComponent] {
        &self.comps
    }

    /// Returns a mutable slice of the calendar properties.
    pub fn components_mut(&mut self) -> &mut [CalComponent] {
        &mut self.comps
    }

    /// Adds the given component to the calendar.
    pub fn add_component(&mut self, comp: CalComponent) {
        self.comps.push(comp);
    }

    /// Deletes the components with given uid from the calendar.
    pub fn delete_components<N: AsRef<str>>(&mut self, uid: N) {
        self.comps.retain(|c| c.uid() != uid.as_ref());
    }

    /// Writes this calendar in iCalendar format into the given writer.
    pub fn write<W: Write>(&self, writer: W) -> io::Result<()> {
        let mut wr = LineWriter::new(writer);
        wr.write_line("BEGIN:VCALENDAR")?;
        for p in self.to_props() {
            wr.write_line(p.to_string())?;
        }
        wr.write_line("END:VCALENDAR")?;
        Ok(())
    }

    fn checked_add(&mut self, comp: CalComponent) {
        // if it's a base component and we already have the same UID, just pretend we don't know it
        if comp.rid().is_none() && self.comps.iter().any(|c| c.uid() == comp.uid()) {
            self.unknown.push(Unknown {
                name: match comp.ctype() {
                    CalCompType::Event => String::from("VEVENT"),
                    CalCompType::Todo => String::from("VTODO"),
                },
                props: comp.to_props(),
            });
        } else {
            self.comps.push(comp);
        }
    }
}

impl PropertyProducer for Calendar {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![];
        props.extend(self.props.iter().cloned());
        for other in &self.unknown {
            props.extend(other.to_props().into_iter());
        }
        for tz in &self.timezones {
            props.extend(tz.to_props().into_iter());
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
    ) -> Result<Self, ParseError>
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
                "BEGIN" if prop.value() == "VTODO" => match CalTodo::from_lines(lines, prop) {
                    Ok(todo) => cal.checked_add(CalComponent::Todo(todo)),
                    Err(e) => warn!("ignoring malformed todo: {}", e),
                },
                "BEGIN" if prop.value() == "VEVENT" => match CalEvent::from_lines(lines, prop) {
                    Ok(ev) => cal.checked_add(CalComponent::Event(ev)),
                    Err(e) => warn!("ignoring malformed event: {}", e),
                },
                "BEGIN" if prop.value() == "VTIMEZONE" => {
                    match CalTimeZone::from_lines(lines, prop) {
                        Ok(tz) => cal.timezones.push(tz),
                        Err(e) => warn!("ignoring malformed timezone: {}", e),
                    }
                }
                "BEGIN" => match Unknown::from_lines(lines, prop) {
                    Ok(other) => cal.unknown.push(other),
                    Err(e) => warn!("ignoring unknown component: {}", e),
                },
                "END" => {
                    if prop.value() != "VCALENDAR" {
                        return Err(ParseError::UnexpectedEnd(prop.take_value()));
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
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut lines = LineReader::new(s.as_bytes());
        let Some(line) = lines.next() else {
            return Err(ParseError::UnexpectedEOF);
        };

        let prop = line.parse::<Property>()?;
        match prop.name().as_str() {
            "BEGIN" if prop.value() == "VCALENDAR" => {
                let cal = Calendar::from_lines(&mut lines, prop)?;
                Ok(cal)
            }
            _ => Err(ParseError::UnexpectedProp(prop.name().to_string())),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct CalTimeZone {
    tzid: String,
    props: Vec<Property>,
}

impl CalTimeZone {
    pub fn new(tzid: String) -> Self {
        Self {
            tzid,
            props: vec![],
        }
    }
}

impl PropertyProducer for CalTimeZone {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![Property::new("BEGIN", vec![], "VTIMEZONE")];
        props.push(Property::new("TZID", vec![], self.tzid.clone()));
        props.extend(self.props.iter().cloned());
        props.push(Property::new("END", vec![], "VTIMEZONE"));
        props
    }
}

impl PropertyConsumer for CalTimeZone {
    fn from_lines<R: BufRead>(lines: &mut LineReader<R>, _: Property) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        let mut tz = CalTimeZone::new("".into());
        loop {
            let Some(line) = lines.next() else {
                break Err(ParseError::UnexpectedEOF);
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                "END" if prop.value() == "VTIMEZONE" => {
                    break Ok(tz);
                }
                "TZID" => tz.tzid = prop.take_value(),
                _ => {
                    tz.props.push(prop);
                }
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Unknown {
    name: String,
    props: Vec<Property>,
}

impl Unknown {
    fn new<N: ToString>(name: N) -> Self {
        Self {
            name: name.to_string(),
            props: Vec::new(),
        }
    }

    fn add(&mut self, prop: Property) {
        self.props.push(prop);
    }
}

impl PropertyProducer for Unknown {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![Property::new("BEGIN", vec![], self.name.clone())];
        props.extend(self.props.iter().cloned());
        props.push(Property::new("END", vec![], self.name.clone()));
        props
    }
}

impl PropertyConsumer for Unknown {
    fn from_lines<R: BufRead>(lines: &mut LineReader<R>, prop: Property) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        let mut other = Unknown::new(prop.take_value());
        loop {
            let Some(line) = lines.next() else {
                break Err(ParseError::UnexpectedEOF);
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
BEGIN:VTIMEZONE
TZID:Europe/Berlin
END:VTIMEZONE
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
BEGIN:VTIMEZONE\r
TZID:Europe/Berlin\r
END:VTIMEZONE\r
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
}
