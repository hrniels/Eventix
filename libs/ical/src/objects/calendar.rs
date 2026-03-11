// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
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

    /// Deletes the components that match the given predicate.
    pub fn delete_components<P>(&mut self, predicate: P)
    where
        P: Fn(&CalComponent) -> bool,
    {
        self.comps.retain(|c| !predicate(c));
    }

    /// Splits this calendar into multiple calendars, one per UID
    pub fn split_by_uid(self) -> Vec<Self> {
        let mut uids = HashMap::<String, Vec<CalComponent>>::new();
        for c in self.comps {
            let entry = uids.entry(c.uid().clone()).or_default();
            entry.push(c);
        }
        uids.into_values()
            .map(|comps| Self {
                comps,
                timezones: self.timezones.clone(),
                props: self.props.clone(),
                unknown: self.unknown.clone(),
            })
            .collect()
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
            let props = comp.to_props();
            // ignore the first and last property as this is BEGIN:*/END:*, which Unknown also adds
            let len = props.len();
            let props = props.into_iter().skip(1).take(len - 2).collect();
            self.unknown.push(Unknown {
                name: match comp.ctype() {
                    CalCompType::Event => String::from("VEVENT"),
                    CalCompType::Todo => String::from("VTODO"),
                },
                props,
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
        for tz in &self.timezones {
            props.extend(tz.to_props().into_iter());
        }
        for comp in &self.comps {
            props.extend(comp.to_props().into_iter());
        }
        // since we also store duplicate components (same UID without RID, see above) in here, they
        // have to go last
        for other in &self.unknown {
            props.extend(other.to_props().into_iter());
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
                    Err(e @ ParseError::UnexpectedEOF) | Err(e @ ParseError::UnexpectedEnd(_)) => {
                        return Err(e);
                    }
                    Err(e) => warn!("ignoring malformed todo: {}", e),
                },
                "BEGIN" if prop.value() == "VEVENT" => match CalEvent::from_lines(lines, prop) {
                    Ok(ev) => cal.checked_add(CalComponent::Event(ev)),
                    Err(e @ ParseError::UnexpectedEOF) | Err(e @ ParseError::UnexpectedEnd(_)) => {
                        return Err(e);
                    }
                    Err(e) => warn!("ignoring malformed event: {}", e),
                },
                "BEGIN" if prop.value() == "VTIMEZONE" => {
                    match CalTimeZone::from_lines(lines, prop) {
                        Ok(tz) => cal.timezones.push(tz),
                        Err(e @ ParseError::UnexpectedEOF)
                        | Err(e @ ParseError::UnexpectedEnd(_)) => return Err(e),
                        Err(e) => warn!("ignoring malformed timezone: {}", e),
                    }
                }
                "BEGIN" => match Unknown::from_lines(lines, prop) {
                    Ok(other) => cal.unknown.push(other),
                    Err(e @ ParseError::UnexpectedEOF) | Err(e @ ParseError::UnexpectedEnd(_)) => {
                        return Err(e);
                    }
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

#[derive(Clone, Debug, Eq, PartialEq)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
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
        objects::{CalComponent, CalDate, CalDateTime, CalTimeZone, Calendar, EventLike},
        parser::{ParseError, Property},
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

    #[test]
    fn malformed_valarm_does_not_leak_end_marker() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:VEVENT\n\
UID:test-uid\n\
DTSTART:20250101T120000Z\n\
BEGIN:VALARM\n\
TRIGGER;VALUE=DATE-TIME:19760401T005545Z\n\
ACTION:NONE\n\
END:VALARM\n\
END:VEVENT\n\
END:VCALENDAR\n";

        // Parsing must succeed
        let cal: Calendar = input.parse().expect("calendar parse failed");

        // Exactly one component, no alarms stored
        assert_eq!(cal.comps.len(), 1, "expected exactly one component");
        match &cal.comps[0] {
            CalComponent::Event(ev) => {
                assert!(ev.alarms().is_none(), "malformed alarm should be ignored");
            }
            _ => panic!("expected VEVENT component"),
        }

        // Serialize
        let mut buf = Vec::new();
        cal.write(&mut buf).expect("serialization failed");
        let serialized = String::from_utf8(buf).expect("invalid utf8 after serialization");

        // Malformed VALARM must not leak into serialized output
        assert!(
            !serialized.contains("BEGIN:VALARM"),
            "Malformed BEGIN:VALARM leaked into output"
        );
        assert!(
            !serialized.contains("END:VALARM"),
            "END:VALARM leaked into parent component"
        );
    }

    #[test]
    fn categories_with_escaped_commas_round_trip() {
        let ical = "BEGIN:VCALENDAR
BEGIN:VTODO
UID:cat-test
DTSTAMP:20250101T000000Z
CATEGORIES:Food\\,Drink,Work
END:VTODO
END:VCALENDAR";

        let cal = ical.parse::<Calendar>().unwrap();
        assert_eq!(cal.comps.len(), 1);
        let CalComponent::Todo(ref todo) = cal.comps[0] else {
            panic!("Expecting TODO");
        };
        assert_eq!(
            todo.categories(),
            Some(["Food,Drink".to_string(), "Work".to_string()].as_slice())
        );

        let mut res = Vec::new();
        let writer = BufWriter::new(&mut res);
        cal.write(writer).unwrap();
        let res = String::from_utf8(res).unwrap();
        assert!(res.contains("CATEGORIES:Food\\,Drink,Work\r"));
    }

    #[test]
    fn malformed_valarm_eof_returns_error() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:VEVENT\n\
UID:test-uid\n\
DTSTART:20250101T120000Z\n\
BEGIN:VALARM\n\
ACTION:NONE\n\
END:VEVENT\n\
END:VCALENDAR\n"; // missing END:VALARM

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedEnd(val)) if val == "VEVENT"),
            "Expected UnexpectedEnd(\"VEVENT\") when END:VALARM is missing"
        );
    }

    #[test]
    fn malformed_valarm_propagates_parse_errors_while_draining() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:VEVENT\n\
UID:test-uid\n\
DTSTART:20250101T120000Z\n\
BEGIN:VALARM\n\
ACTION:NONE\n\
BAD\x01LINE\n\
END:VALARM\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedEnd(val)) if val == "VALARM"),
            "Expected UnexpectedEnd(\"VALARM\") to propagate"
        );
    }

    #[test]
    fn vevent_unexpected_eof_is_fatal() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:test\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedEOF)),
            "Expected UnexpectedEOF for incomplete VEVENT"
        );
    }

    #[test]
    fn vevent_wrong_end_is_fatal() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:test\n\
END:VTODO\n\
END:VCALENDAR\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedEOF)),
            "Expected UnexpectedEOF for wrong VEVENT end"
        );
    }

    #[test]
    fn vtodo_unexpected_eof_is_fatal() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTODO\n\
UID:test\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedEOF)),
            "Expected UnexpectedEOF for incomplete VTODO"
        );
    }

    #[test]
    fn vtodo_wrong_end_is_fatal() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTODO\n\
UID:test\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedEnd(val)) if val == "VEVENT"),
            "Expected UnexpectedEnd(\"VEVENT\") for wrong VTODO end"
        );
    }

    #[test]
    fn vtimezone_unexpected_eof_is_fatal() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedEOF)),
            "Expected UnexpectedEOF for incomplete VTIMEZONE"
        );
    }

    #[test]
    fn vtimezone_wrong_end_is_fatal() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedEOF)),
            "Expected UnexpectedEOF for wrong VTIMEZONE end"
        );
    }

    #[test]
    fn unknown_component_unexpected_eof_is_fatal() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:XFOO\n\
BAR:1\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedEOF)),
            "Expected UnexpectedEOF for incomplete unknown component"
        );
    }

    #[test]
    fn unknown_component_wrong_end_is_fatal() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:XFOO\n\
END:XBAR\n\
END:VCALENDAR\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedEOF)),
            "Expected UnexpectedEOF for wrong unknown component end"
        );
    }

    #[test]
    fn missing_begin_vcalendar_is_fatal() {
        let input = "BEGIN:VEVENT\nUID:test\nEND:VEVENT\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedProp(val)) if val == "BEGIN"),
            "Expected UnexpectedProp(\"BEGIN\") when VCALENDAR is missing"
        );
    }

    #[test]
    fn unexpected_end_before_vcalendar_is_fatal() {
        let input = "END:VEVENT\n";

        let res = input.parse::<Calendar>();
        assert!(
            matches!(res, Err(ParseError::UnexpectedProp(val)) if val == "END"),
            "Expected UnexpectedProp(\"END\") before VCALENDAR"
        );
    }

    #[test]
    fn properties_returns_calendar_properties() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
PRODID:-//Test//Test//EN\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        let props = cal.properties();
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].name(), "VERSION");
        assert_eq!(props[0].value(), "2.0");
        assert_eq!(props[1].name(), "PRODID");
        assert_eq!(props[1].value(), "-//Test//Test//EN");
    }

    #[test]
    fn timezones_returns_timezone_components() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:VTIMEZONE\n\
TZID:America/New_York\n\
END:VTIMEZONE\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/London\n\
END:VTIMEZONE\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        let tzs = cal.timezones();
        assert_eq!(tzs.len(), 2);
        assert_eq!(tzs[0].tzid, "America/New_York");
        assert_eq!(tzs[1].tzid, "Europe/London");
    }

    #[test]
    fn add_timezone_adds_timezone_to_calendar() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        assert!(cal.timezones().is_empty());

        let tz = CalTimeZone::new("Asia/Tokyo".to_string());
        cal.add_timezone(tz.clone());

        let tzs = cal.timezones();
        assert_eq!(tzs.len(), 1);
        assert_eq!(tzs[0].tzid, "Asia/Tokyo");
    }

    #[test]
    fn delete_components_removes_matching_components() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:VEVENT\n\
UID:keep-uid\n\
DTSTART:20250101T120000Z\n\
SUMMARY:Keep Me\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:delete-uid\n\
DTSTART:20250102T120000Z\n\
SUMMARY:Delete Me\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        {
            let comps = cal.components_mut();
            assert_eq!(comps.len(), 2);
            let _ = comps.first_mut();
        }

        cal.delete_components(|c| {
            if let CalComponent::Event(ev) = c {
                ev.summary().map(|s| s.contains("Delete")).unwrap_or(false)
            } else {
                false
            }
        });

        assert_eq!(cal.components().len(), 1);
        let CalComponent::Event(ev) = &cal.components()[0] else {
            panic!("Expected Event");
        };
        assert_eq!(ev.uid().as_str(), "keep-uid");
    }

    #[test]
    fn split_by_uid_splits_into_multiple_calendars() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
PRODID:-//Test//Test//EN\n\
BEGIN:VEVENT\n\
UID:uid-1\n\
DTSTART:20250101T120000Z\n\
SUMMARY:Event 1\n\
END:VEVENT\n\
BEGIN:VTODO\n\
UID:uid-2\n\
DTSTART:20250101T120000Z\n\
SUMMARY:Todo 1\n\
END:VTODO\n\
BEGIN:VTODO\n\
UID:uid-3\n\
DTSTART:20250102T120000Z\n\
SUMMARY:Todo 2\n\
END:VTODO\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
END:VTIMEZONE\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        let splits = cal.split_by_uid();

        assert_eq!(splits.len(), 3);

        for split in &splits {
            assert_eq!(split.components().len(), 1);
            assert_eq!(split.timezones().len(), 1);
            assert_eq!(split.properties().len(), 2);
        }

        let uids: Vec<_> = splits
            .iter()
            .map(|c| c.components()[0].uid().as_str().to_string())
            .collect();
        assert!(uids.contains(&"uid-1".to_string()));
        assert!(uids.contains(&"uid-2".to_string()));
        assert!(uids.contains(&"uid-3".to_string()));
    }

    #[test]
    fn duplicate_event_without_rid_is_stored_as_unknown() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:VEVENT\n\
UID:duplicate-uid\n\
DTSTART:20250101T120000Z\n\
SUMMARY:First Event\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:duplicate-uid\n\
DTSTART:20250102T120000Z\n\
SUMMARY:Duplicate Event\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        assert_eq!(cal.components().len(), 1);
        assert_eq!(cal.unknown.len(), 1);

        let unknown = &cal.unknown[0];
        assert_eq!(unknown.name, "VEVENT");
    }

    #[test]
    fn duplicate_todo_without_rid_is_stored_as_unknown() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:VTODO\n\
UID:todo-uid\n\
DTSTART:20250101T120000Z\n\
SUMMARY:First Todo\n\
END:VTODO\n\
BEGIN:VTODO\n\
UID:todo-uid\n\
DTSTART:20250102T120000Z\n\
SUMMARY:Duplicate Todo\n\
END:VTODO\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        assert_eq!(cal.components().len(), 1);
        assert_eq!(cal.unknown.len(), 1);

        let unknown = &cal.unknown[0];
        assert_eq!(unknown.name, "VTODO");
    }

    #[test]
    fn unknown_component_round_trip() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:X-CUSTOM\n\
X-PROP:custom-value\n\
END:X-CUSTOM\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        assert_eq!(cal.unknown.len(), 1);
        assert_eq!(cal.unknown[0].name, "X-CUSTOM");

        let mut buf = Vec::new();
        cal.write(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("BEGIN:X-CUSTOM"));
        assert!(output.contains("X-PROP:custom-value"));
        assert!(output.contains("END:X-CUSTOM"));
    }

    #[test]
    fn timezone_serialization_includes_props() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:VTIMEZONE\n\
TZID:America/Chicago\n\
X-CUSTOM-PROP:custom-value\n\
END:VTIMEZONE\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        let tz = &cal.timezones()[0];
        assert_eq!(tz.tzid, "America/Chicago");
        assert_eq!(tz.props.len(), 1);
        assert_eq!(tz.props[0].name(), "X-CUSTOM-PROP");

        let mut buf = Vec::new();
        cal.write(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let expected = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:America/Chicago\r\n\
X-CUSTOM-PROP:custom-value\r\n\
END:VTIMEZONE\r\n\
END:VCALENDAR\r\n";
        assert_eq!(output, expected);
    }
}
