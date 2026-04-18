// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use chrono_tz::Tz;
use tracing::warn;

use crate::objects::{
    CalCompType, CalComponent, CalDate, CalDateTime, CalEvent, CalTimeZone, CalTodo, CalTrigger,
    CalendarTimeZoneResolver, DateContext, EventLike,
};
use crate::parser::{
    LineReader, LineWriter, ParseError, Property, PropertyConsumer, PropertyProducer,
};

/// Represents an iCalendar object.
///
/// Such a calendar consists of one or more [`CalComponent`]s, each being either an event or TODO.
/// Additionally, the calendar itself can have properties such as the version or product id.
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.4>.
#[derive(Clone, Debug)]
pub struct Calendar {
    comps: Vec<CalComponent>,
    timezones: Vec<CalTimeZone>,
    props: Vec<Property>,
    unknown: Vec<Unknown>,
    tzresolver: OnceLock<Arc<CalendarTimeZoneResolver>>,
}

impl Calendar {
    fn invalidate_timezone_resolver(&mut self) {
        self.tzresolver = OnceLock::new();
    }

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
        self.invalidate_timezone_resolver();
    }

    /// Returns a slice of the calendar components.
    pub fn components(&self) -> &[CalComponent] {
        &self.comps
    }

    /// Returns a mutable slice of the calendar properties.
    pub fn components_mut(&mut self) -> &mut [CalComponent] {
        self.invalidate_timezone_resolver();
        &mut self.comps
    }

    pub fn timezone_resolver(&self) -> &CalendarTimeZoneResolver {
        self.tzresolver
            .get_or_init(|| Arc::new(CalendarTimeZoneResolver::new(self)))
            .as_ref()
    }

    /// Returns the cached timezone resolver as a shared handle.
    pub fn timezone_resolver_arc(&self) -> Arc<CalendarTimeZoneResolver> {
        self.tzresolver
            .get_or_init(|| Arc::new(CalendarTimeZoneResolver::new(self)))
            .clone()
    }

    /// Builds a reusable date resolution context for the given fallback timezone.
    pub fn date_context(&self, fallback: Tz) -> DateContext {
        DateContext::new(self.timezone_resolver_arc(), fallback)
    }

    /// Adds the given component to the calendar.
    pub fn add_component(&mut self, comp: CalComponent) {
        self.comps.push(comp);
        self.invalidate_timezone_resolver();
    }

    /// Deletes the components that match the given predicate.
    pub fn delete_components<P>(&mut self, predicate: P)
    where
        P: Fn(&CalComponent) -> bool,
    {
        self.comps.retain(|c| !predicate(c));
        self.invalidate_timezone_resolver();
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
                tzresolver: OnceLock::new(),
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

    /// Populates missing VTIMEZONE entries for all used TZIDs.
    pub fn populate_timezones(&mut self) {
        let tzids = self.used_tzids();
        let used: HashSet<_> = tzids.iter().map(String::as_str).collect();
        self.timezones.retain(|tz| used.contains(tz.tzid()));

        for tzid in tzids {
            if self.timezones.iter().any(|tz| tz.tzid() == tzid) {
                continue;
            }

            match CalTimeZone::from_chrono_tz(&tzid) {
                Some(tz) => self.timezones.push(tz),
                None => warn!("failed to populate VTIMEZONE for {}", tzid),
            }
        }

        self.invalidate_timezone_resolver();
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
        self.invalidate_timezone_resolver();
    }

    /// Validates all component dates against the given local timezone.
    ///
    /// Components whose dates fall in a DST gap (non-existent time) or a DST fold (ambiguous
    /// time) are removed and a warning is logged.
    ///
    /// Returns true if no components were removed.
    pub fn validate_times(&mut self, local_tz: &Tz) -> bool {
        let len = self.comps.len();
        let comps = std::mem::take(&mut self.comps);
        let resolver = self.timezone_resolver();
        self.comps = comps
            .into_iter()
            .filter(|comp| {
                if let Err(e) = Self::validate_component(comp, local_tz, resolver) {
                    warn!(
                        "ignoring component {} (uid {}): {}",
                        comp.ctype(),
                        comp.uid(),
                        e
                    );
                    return false;
                }
                true
            })
            .collect();
        self.comps.len() == len
    }

    fn validate_component(
        comp: &CalComponent,
        local_tz: &Tz,
        resolver: &CalendarTimeZoneResolver,
    ) -> Result<(), ParseError> {
        Self::validate_eventlike_dates(comp, local_tz, resolver)?;
        match comp {
            CalComponent::Event(ev) => {
                Self::validate_opt_date(ev.end(), local_tz, resolver)?;
            }
            CalComponent::Todo(td) => {
                Self::validate_opt_date(td.due(), local_tz, resolver)?;
                Self::validate_opt_date(td.completed(), local_tz, resolver)?;
            }
        }
        Ok(())
    }

    fn validate_eventlike_dates(
        comp: &CalComponent,
        local_tz: &Tz,
        resolver: &CalendarTimeZoneResolver,
    ) -> Result<(), ParseError> {
        Self::validate_opt_date(comp.start(), local_tz, resolver)?;
        Self::validate_opt_date(comp.created(), local_tz, resolver)?;
        Self::validate_opt_date(comp.last_modified(), local_tz, resolver)?;
        comp.stamp().validate_with(local_tz, resolver)?;
        Self::validate_opt_date(comp.rid(), local_tz, resolver)?;
        for exdate in comp.exdates() {
            exdate.validate_with(local_tz, resolver)?;
        }
        if let Some(alarms) = comp.alarms() {
            for alarm in alarms {
                if let CalTrigger::Absolute(date) = alarm.trigger() {
                    date.validate_with(local_tz, resolver)?;
                }
            }
        }
        if let Some(rrule) = comp.rrule() {
            Self::validate_opt_date(rrule.until(), local_tz, resolver)?;
        }
        Ok(())
    }

    fn validate_opt_date(
        date: Option<&CalDate>,
        local_tz: &Tz,
        resolver: &CalendarTimeZoneResolver,
    ) -> Result<(), ParseError> {
        if let Some(d) = date {
            d.validate_with(local_tz, resolver)?;
        }
        Ok(())
    }

    fn used_tzids(&self) -> Vec<String> {
        let mut tzids = HashSet::new();

        for comp in &self.comps {
            Self::add_date_tzid(comp.start(), &mut tzids);
            Self::add_date_tzid(comp.created(), &mut tzids);
            Self::add_date_tzid(comp.last_modified(), &mut tzids);
            Self::add_date_tzid(Some(comp.stamp()), &mut tzids);
            Self::add_date_tzid(comp.rid(), &mut tzids);
            Self::add_date_tzid(comp.end_or_due(), &mut tzids);

            for exdate in comp.exdates() {
                Self::add_date_tzid(Some(exdate), &mut tzids);
            }

            if let Some(alarms) = comp.alarms() {
                for alarm in alarms {
                    if let CalTrigger::Absolute(date) = alarm.trigger() {
                        Self::add_date_tzid(Some(date), &mut tzids);
                    }
                }
            }

            if let CalComponent::Todo(td) = comp {
                Self::add_date_tzid(td.completed(), &mut tzids);
            }
        }

        let mut tzids: Vec<_> = tzids.into_iter().collect();
        tzids.sort();
        tzids
    }

    fn add_date_tzid(date: Option<&CalDate>, tzids: &mut HashSet<String>) {
        if let Some(CalDate::DateTime(CalDateTime::Timezone(_, tzid))) = date {
            tzids.insert(tzid.clone());
        }
    }
}

impl Default for Calendar {
    fn default() -> Self {
        Self {
            comps: Vec::new(),
            timezones: Vec::new(),
            props: Vec::new(),
            unknown: Vec::new(),
            tzresolver: OnceLock::new(),
        }
    }
}

impl PartialEq for Calendar {
    fn eq(&self, other: &Self) -> bool {
        self.comps == other.comps
            && self.timezones == other.timezones
            && self.props == other.props
            && self.unknown == other.unknown
    }
}

impl Eq for Calendar {}

impl PropertyProducer for Calendar {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![];
        props.extend(self.props.iter().cloned());
        for tz in &self.timezones {
            props.extend(tz.to_props());
        }
        for comp in &self.comps {
            props.extend(comp.to_props());
        }
        // since we also store duplicate components (same UID without RID, see above) in here, they
        // have to go last
        for other in &self.unknown {
            props.extend(other.to_props());
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
        let mut pending = None;
        loop {
            let line = if let Some(line) = pending.take() {
                line
            } else {
                let Some(line) = lines.next() else {
                    break Ok(cal);
                };
                line
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                "BEGIN" if prop.value() == "VTIMEZONE" => {
                    let buffered = buffer_timezone(lines);
                    pending = buffered.pending;

                    if !buffered.complete {
                        warn!("ignoring malformed timezone: unterminated VTIMEZONE");
                        continue;
                    }

                    let mut timezone_lines = buffered.lines.join("\n");
                    timezone_lines.push('\n');
                    let mut timezone_reader = LineReader::new(timezone_lines.as_bytes());

                    match CalTimeZone::from_lines(&mut timezone_reader, prop) {
                        Ok(tz) => cal.timezones.push(tz),
                        Err(e) => warn!("ignoring malformed timezone: {}", e),
                    }
                }
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

struct BufferedTimeZone {
    lines: Vec<String>,
    pending: Option<String>,
    complete: bool,
}

fn buffer_timezone<R: BufRead>(lines: &mut LineReader<R>) -> BufferedTimeZone {
    let mut buffered = Vec::new();
    let mut depth = 0usize;

    loop {
        let Some(line) = lines.next() else {
            return BufferedTimeZone {
                lines: buffered,
                pending: None,
                complete: false,
            };
        };

        match line.parse::<Property>() {
            Ok(prop) if depth == 0 => match prop.name().as_str() {
                "END" if prop.value() == "VTIMEZONE" => {
                    buffered.push(line);
                    return BufferedTimeZone {
                        lines: buffered,
                        pending: None,
                        complete: true,
                    };
                }
                "BEGIN" if prop.value() == "STANDARD" || prop.value() == "DAYLIGHT" => {
                    depth = 1;
                    buffered.push(line);
                }
                "BEGIN" => {
                    return BufferedTimeZone {
                        lines: buffered,
                        pending: Some(line),
                        complete: false,
                    };
                }
                "END" if prop.value() == "VCALENDAR" => {
                    return BufferedTimeZone {
                        lines: buffered,
                        pending: Some(line),
                        complete: false,
                    };
                }
                _ => buffered.push(line),
            },
            Ok(prop) => {
                if prop.name() == "BEGIN" {
                    depth += 1;
                } else if prop.name() == "END" {
                    depth = depth.saturating_sub(1);
                }
                buffered.push(line);
            }
            Err(_) => buffered.push(line),
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

    use chrono_tz::Tz;

    use crate::{
        objects::{CalComponent, CalDate, CalDateTime, Calendar, EventLike},
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
BEGIN:STANDARD
DTSTART:19701025T030000
TZOFFSETFROM:+0200
TZOFFSETTO:+0100
TZNAME:CET
END:STANDARD
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
BEGIN:STANDARD\r
DTSTART:19701025T030000\r
TZOFFSETFROM:+0200\r
TZOFFSETTO:+0100\r
TZNAME:CET\r
END:STANDARD\r
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
    fn add_timezone_adds_timezone_to_calendar() {
        let input = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        assert!(cal.timezones().is_empty());

        let parsed = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Asia/Tokyo\n\
BEGIN:STANDARD\n\
DTSTART:19700101T000000\n\
TZOFFSETFROM:+0900\n\
TZOFFSETTO:+0900\n\
END:STANDARD\n\
END:VTIMEZONE\n\
END:VCALENDAR\n"
            .parse::<Calendar>()
            .unwrap();
        cal.add_timezone(parsed.timezones()[0].clone());

        let tzs = cal.timezones();
        assert_eq!(tzs.len(), 1);
        assert_eq!(tzs[0].tzid(), "Asia/Tokyo");
    }

    #[test]
    fn populate_timezones_adds_missing_used_timezone() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:test\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Europe/Berlin:20250330T100000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        assert!(cal.timezones().is_empty());

        cal.populate_timezones();

        assert_eq!(cal.timezones().len(), 1);
        assert_eq!(cal.timezones()[0].tzid(), "Europe/Berlin");
        assert!(!cal.timezones()[0].observances().is_empty());
    }

    #[test]
    fn populate_timezones_does_not_replace_existing_timezone() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
BEGIN:STANDARD\n\
DTSTART:19701025T030000\n\
TZOFFSETFROM:+0200\n\
TZOFFSETTO:+0100\n\
TZNAME:CUSTOM\n\
END:STANDARD\n\
END:VTIMEZONE\n\
BEGIN:VEVENT\n\
UID:test\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Europe/Berlin:20250330T100000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        cal.populate_timezones();

        assert_eq!(cal.timezones().len(), 1);
        assert_eq!(
            cal.timezones()[0].observances()[0].tzname(),
            ["CUSTOM".to_string()].as_slice()
        );
    }

    #[test]
    fn populate_timezones_removes_unused_timezone() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
BEGIN:STANDARD\n\
DTSTART:19701025T030000\n\
TZOFFSETFROM:+0200\n\
TZOFFSETTO:+0100\n\
END:STANDARD\n\
END:VTIMEZONE\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        assert_eq!(cal.timezones().len(), 1);

        cal.populate_timezones();

        assert!(cal.timezones().is_empty());
    }

    #[test]
    fn populate_timezones_prunes_unused_and_keeps_used_existing_timezone() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
BEGIN:STANDARD\n\
DTSTART:19701025T030000\n\
TZOFFSETFROM:+0200\n\
TZOFFSETTO:+0100\n\
TZNAME:BERLIN\n\
END:STANDARD\n\
END:VTIMEZONE\n\
BEGIN:VTIMEZONE\n\
TZID:America/New_York\n\
BEGIN:STANDARD\n\
DTSTART:19701101T020000\n\
TZOFFSETFROM:-0400\n\
TZOFFSETTO:-0500\n\
TZNAME:NEWYORK\n\
END:STANDARD\n\
END:VTIMEZONE\n\
BEGIN:VEVENT\n\
UID:test\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Europe/Berlin:20250330T100000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        assert_eq!(cal.timezones().len(), 2);

        cal.populate_timezones();

        assert_eq!(cal.timezones().len(), 1);
        assert_eq!(cal.timezones()[0].tzid(), "Europe/Berlin");
        assert_eq!(
            cal.timezones()[0].observances()[0].tzname(),
            ["BERLIN".to_string()].as_slice()
        );
    }

    #[test]
    fn malformed_timezone_is_ignored_and_can_be_repopulated() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
END:VTIMEZONE\n\
BEGIN:VEVENT\n\
UID:test\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Europe/Berlin:20250330T100000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        assert!(cal.timezones().is_empty());

        cal.populate_timezones();

        assert_eq!(cal.timezones().len(), 1);
        assert_eq!(cal.timezones()[0].tzid(), "Europe/Berlin");
    }

    #[test]
    fn timezone_resolver_prefers_embedded_vtimezone_over_system_tzdb() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
BEGIN:STANDARD\n\
DTSTART:19700101T000000\n\
TZOFFSETFROM:+0300\n\
TZOFFSETTO:+0300\n\
TZNAME:CUSTOM\n\
END:STANDARD\n\
END:VTIMEZONE\n\
BEGIN:VEVENT\n\
UID:test\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Europe/Berlin:20250330T100000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        let resolver = cal.timezone_resolver();
        let start = cal.components()[0].start().unwrap();
        let resolved = start.as_start_with_resolver(&chrono_tz::UTC, &resolver);

        assert_eq!(
            resolved.naive_local(),
            "2025-03-30T10:00:00".parse().unwrap()
        );
        assert_eq!(resolved.offset().local_minus_utc(), 3 * 3600);
    }

    #[test]
    fn timezone_resolver_falls_back_to_system_tzdb_without_embedded_vtimezone() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:test\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Europe/Berlin:20250330T100000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        let resolver = cal.timezone_resolver();
        let start = cal.components()[0].start().unwrap();
        let resolved = start.as_start_with_resolver(&chrono_tz::UTC, &resolver);

        assert_eq!(
            resolved.naive_local(),
            "2025-03-30T10:00:00".parse().unwrap()
        );
        assert_eq!(resolved.offset().local_minus_utc(), 2 * 3600);
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
    fn vtimezone_unexpected_eof_is_ignored() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n";

        let cal = input.parse::<Calendar>().unwrap();
        assert!(cal.timezones().is_empty());
    }

    #[test]
    fn vtimezone_wrong_end_is_ignored() {
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTIMEZONE\n\
TZID:Europe/Berlin\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let cal = input.parse::<Calendar>().unwrap();
        assert!(cal.timezones().is_empty());
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
BEGIN:STANDARD\n\
DTSTART:19701025T030000\n\
TZOFFSETFROM:+0200\n\
TZOFFSETTO:+0100\n\
END:STANDARD\n\
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

    // --- validate_times ---

    #[test]
    fn validate_times_removes_event_with_dst_gap_start() {
        // 2:30 AM on 2025-03-30 doesn't exist in Europe/Berlin (spring forward).
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:gap-ev\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Europe/Berlin:20250330T023000\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:ok-ev\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Europe/Berlin:20250330T100000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        assert_eq!(cal.components().len(), 2);

        cal.validate_times(&Tz::Europe__Berlin);

        assert_eq!(cal.components().len(), 1);
        assert_eq!(cal.components()[0].uid().as_str(), "ok-ev");
    }

    #[test]
    fn validate_times_removes_todo_with_dst_gap_due() {
        // 2:30 AM on 2025-03-09 doesn't exist in America/New_York (spring
        // forward).
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTODO\n\
UID:gap-td\n\
DTSTAMP:20250101T000000Z\n\
DUE;TZID=America/New_York:20250309T023000\n\
END:VTODO\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        assert_eq!(cal.components().len(), 1);

        cal.validate_times(&Tz::America__New_York);

        assert!(cal.components().is_empty());
    }

    #[test]
    fn validate_times_keeps_utc_components() {
        // UTC times are always valid, even if local_tz has a gap at the same
        // wall-clock time.
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:utc-ev\n\
DTSTAMP:20250101T000000Z\n\
DTSTART:20250330T023000Z\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        cal.validate_times(&Tz::Europe__Berlin);

        assert_eq!(cal.components().len(), 1);
    }

    #[test]
    fn validate_times_removes_event_with_floating_dst_gap() {
        // Floating times are checked against local_tz.
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:float-gap\n\
DTSTAMP:20250101T000000Z\n\
DTSTART:20250330T023000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        cal.validate_times(&Tz::Europe__Berlin);

        assert!(cal.components().is_empty());
    }

    #[test]
    fn validate_times_removes_event_with_dst_fold_start() {
        // 2:30 AM on 2025-10-26 is ambiguous in Europe/Berlin (fall back).
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:fold-ev\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Europe/Berlin:20251026T023000\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:ok-ev\n\
DTSTAMP:20250101T000000Z\n\
DTSTART;TZID=Europe/Berlin:20251026T040000\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        assert_eq!(cal.components().len(), 2);

        cal.validate_times(&Tz::Europe__Berlin);

        assert_eq!(cal.components().len(), 1);
        assert_eq!(cal.components()[0].uid().as_str(), "ok-ev");
    }

    #[test]
    fn validate_times_removes_todo_with_dst_fold_due() {
        // 1:30 AM on 2025-11-02 is ambiguous in America/New_York (fall back).
        let input = "BEGIN:VCALENDAR\n\
BEGIN:VTODO\n\
UID:fold-td\n\
DTSTAMP:20250101T000000Z\n\
DUE;TZID=America/New_York:20251102T013000\n\
END:VTODO\n\
END:VCALENDAR\n";

        let mut cal = input.parse::<Calendar>().unwrap();
        assert_eq!(cal.components().len(), 1);

        cal.validate_times(&Tz::America__New_York);

        assert!(cal.components().is_empty());
    }
}
