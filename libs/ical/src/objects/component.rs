// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::DateTime;
use chrono_tz::Tz;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::io::BufRead;
use tracing::warn;

use crate::objects::{
    CalAlarm, CalAttendee, CalDate, CalDuration, CalEvent, CalOrganizer, CalRRule, CalTodo,
    EventLike, UpdatableEventLike,
};
use crate::parser::{LineReader, ParseError, Property, PropertyConsumer, PropertyProducer};
use crate::util;

use super::recur::RecurIterator;

/// Represents the low priority (9)
pub const PRIORITY_LOW: u8 = 9;
/// Represents the medium priority (5)
pub const PRIORITY_MEDIUM: u8 = 5;
/// Represents the high priority (1)
pub const PRIORITY_HIGH: u8 = 1;

/// Common parts of events and TODOs.
///
/// As events and TODOs share many properties and behaviours, these are captured in this struct.
/// For example, both have a UID, summary, can be recurrent, and so on.
#[derive(Debug, Eq, PartialEq)]
pub struct EventLikeComponent {
    ctype: CalCompType,
    uid: String,
    stamp: CalDate,
    created: Option<CalDate>,
    last_mod: Option<CalDate>,
    start: Option<CalDate>,
    duration: Option<CalDuration>,
    summary: Option<String>,
    desc: Option<String>,
    location: Option<String>,
    categories: Option<Vec<String>>,
    organizer: Option<CalOrganizer>,
    attendees: Option<Vec<CalAttendee>>,
    exdates: Vec<CalDate>,
    alarms: Option<Vec<CalAlarm>>,
    // 0 = undefined; 1 = highest, 9 = lowest
    priority: Option<u8>,
    rrule: Option<CalRRule>,
    rid: Option<CalDate>,
    props: Vec<Property>,
}

impl EventLikeComponent {
    pub(crate) fn new_empty(ctype: CalCompType) -> Self {
        Self {
            ctype,
            uid: String::from(""),
            stamp: CalDate::default(),
            created: None,
            last_mod: None,
            start: None,
            duration: None,
            summary: None,
            desc: None,
            location: None,
            categories: None,
            organizer: None,
            attendees: None,
            exdates: vec![],
            alarms: None,
            priority: None,
            rrule: None,
            rid: None,
            props: vec![],
        }
    }

    /// Creates a new object with given UID and type.
    ///
    /// Note that the stamp, creation date, and last-modification date are all set to
    /// `CalDate::now`.
    pub fn new<T: ToString>(uid: T, ctype: CalCompType) -> Self {
        let mut new = Self::new_empty(ctype);
        new.uid = uid.to_string();
        new.stamp = CalDate::now();
        new.created = Some(CalDate::now());
        new.last_mod = Some(CalDate::now());
        new
    }

    /// Sets the start of the component.
    pub fn set_start(&mut self, start: Option<CalDate>) {
        self.start = start;
    }

    pub(crate) fn parse_prop<R: BufRead>(
        &mut self,
        lines: &mut LineReader<R>,
        prop: Property,
    ) -> Result<(), ParseError> {
        match prop.name().as_str() {
            "UID" => {
                self.uid = prop.take_value();
            }
            "CREATED" => {
                self.created = Some(prop.try_into()?);
            }
            "LAST-MODIFIED" => {
                self.last_mod = Some(prop.try_into()?);
            }
            "DTSTAMP" => {
                self.stamp = prop.try_into()?;
            }
            "DTSTART" => {
                self.start = Some(prop.try_into()?);
            }
            "DURATION" => {
                self.duration = Some(prop.take_value().parse()?);
            }
            "SUMMARY" => {
                self.summary = Some(prop.take_value());
            }
            "DESCRIPTION" => {
                self.desc = Some(prop.take_value());
            }
            "LOCATION" => {
                self.location = Some(prop.take_value());
            }
            "CATEGORIES" => {
                self.categories = Some(
                    util::split_escaped_commas(prop.value())
                        .into_iter()
                        .map(|v| v.trim().to_string())
                        .collect(),
                );
            }
            "ORGANIZER" => {
                self.organizer = Some(prop.try_into()?);
            }
            "ATTENDEE" => {
                let att: CalAttendee = prop.try_into()?;
                if self.attendees.is_none() {
                    self.attendees = Some(vec![]);
                }
                let attendees = self.attendees.as_mut().unwrap();
                // since some implementations store multiple ATTENDEE properties for the same
                // participant and also with different properties, we merge additional occurrences
                // for the same address with the previous one.
                if let Some(ex_att) = attendees.iter_mut().find(|a| a.address() == att.address()) {
                    ex_att.merge_with(att);
                } else {
                    attendees.push(att);
                }
            }
            "EXDATE" => {
                for date in prop.value().split(',') {
                    let dateprop = Property::new(prop.name(), prop.params().to_vec(), date);
                    self.exdates.push(dateprop.try_into()?);
                }
            }
            "PRIORITY" => {
                let prio = prop.value().parse()?;
                if prio >= 10 {
                    return Err(ParseError::InvalidPriority(prio));
                }
                self.priority = Some(prio);
            }
            "RRULE" => {
                self.rrule = Some(prop.value().parse()?);
            }
            "RECURRENCE-ID" => {
                self.rid = Some(prop.try_into()?);
            }
            "BEGIN" => {
                if prop.value() != "VALARM" {
                    return Err(ParseError::UnexpectedBegin(prop.take_value()));
                }
                match CalAlarm::from_lines(lines, prop) {
                    Ok(alarm) => {
                        if self.alarms.is_none() {
                            self.alarms = Some(vec![]);
                        }
                        self.alarms.as_mut().unwrap().push(alarm);
                    }
                    Err(e) => {
                        warn!("ignoring malformed alarm: {}", e);
                        // Drain remaining lines until matching END:VALARM
                        loop {
                            let Some(line) = lines.next() else {
                                return Err(ParseError::UnexpectedEOF);
                            };
                            let prop = line.parse::<Property>()?;
                            if prop.name() == "END" {
                                if prop.value() == "VALARM" {
                                    break;
                                } else {
                                    return Err(ParseError::UnexpectedEnd(prop.take_value()));
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                self.props.push(prop);
            }
        }
        Ok(())
    }
}

impl PropertyProducer for EventLikeComponent {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![];
        props.push(Property::new("UID", vec![], self.uid.clone()));
        if let Some(ref created) = self.created {
            props.push(created.to_prop("CREATED"));
        }
        if let Some(ref last_mod) = self.last_mod {
            props.push(last_mod.to_prop("LAST-MODIFIED"));
        }
        props.push(self.stamp.to_prop("DTSTAMP"));
        if let Some(ref dtstart) = self.start {
            props.push(dtstart.to_prop("DTSTART"));
        }
        if let Some(ref dur) = self.duration {
            props.push(Property::new("DURATION", vec![], dur.to_string()));
        }
        if let Some(ref summary) = self.summary {
            props.push(Property::new("SUMMARY", vec![], summary.clone()));
        }
        if let Some(ref desc) = self.desc {
            props.push(Property::new("DESCRIPTION", vec![], desc.clone()));
        }
        if let Some(ref loc) = self.location {
            props.push(Property::new("LOCATION", vec![], loc.clone()));
        }
        if let Some(ref cats) = self.categories {
            props.push(Property::new_escaped(
                "CATEGORIES",
                vec![],
                cats.iter().map(|c| util::escape_text(c)).join(","),
            ));
        }
        if let Some(ref org) = self.organizer {
            props.push(org.to_prop());
        }
        if let Some(ref atts) = self.attendees {
            props.extend(atts.iter().map(|a| a.to_prop()));
        }
        for exdate in &self.exdates {
            props.push(exdate.to_prop("EXDATE"));
        }
        if let Some(prio) = self.priority {
            props.push(Property::new("PRIORITY", vec![], format!("{prio}")));
        }
        if let Some(rrule) = &self.rrule {
            props.push(Property::new_escaped("RRULE", vec![], format!("{rrule}")));
        }
        if let Some(ref rid) = self.rid {
            props.push(rid.to_prop("RECURRENCE-ID"));
        }
        if let Some(ref alarms) = self.alarms {
            for a in alarms {
                props.extend(a.to_props().into_iter());
            }
        }
        props.extend(self.props.iter().cloned());
        props
    }
}

impl EventLike for EventLikeComponent {
    fn ctype(&self) -> CalCompType {
        self.ctype
    }

    fn uid(&self) -> &String {
        &self.uid
    }

    fn stamp(&self) -> &CalDate {
        &self.stamp
    }

    fn created(&self) -> Option<&CalDate> {
        self.created.as_ref()
    }

    fn last_modified(&self) -> Option<&CalDate> {
        self.last_mod.as_ref()
    }

    fn start(&self) -> Option<&CalDate> {
        self.start.as_ref()
    }

    fn end_or_due(&self) -> Option<&CalDate> {
        None
    }

    fn duration(&self) -> Option<&CalDuration> {
        self.duration.as_ref()
    }

    fn summary(&self) -> Option<&String> {
        self.summary.as_ref()
    }

    fn description(&self) -> Option<&String> {
        self.desc.as_ref()
    }

    fn location(&self) -> Option<&String> {
        self.location.as_ref()
    }

    fn categories(&self) -> Option<&[String]> {
        self.categories.as_ref().map(|c| c.as_ref())
    }

    fn organizer(&self) -> Option<&CalOrganizer> {
        self.organizer.as_ref()
    }

    fn attendees(&self) -> Option<&[CalAttendee]> {
        self.attendees.as_ref().map(|a| a.as_ref())
    }

    fn exdates(&self) -> &[CalDate] {
        &self.exdates
    }

    fn alarms(&self) -> Option<&[CalAlarm]> {
        self.alarms.as_ref().map(|a| a.as_ref())
    }

    fn rrule(&self) -> Option<&CalRRule> {
        self.rrule.as_ref()
    }

    fn rid(&self) -> Option<&CalDate> {
        self.rid.as_ref()
    }

    fn priority(&self) -> Option<u8> {
        self.priority
    }
}

impl UpdatableEventLike for EventLikeComponent {
    fn set_start(&mut self, start: Option<CalDate>) {
        self.start = start;
    }

    fn set_summary(&mut self, summary: Option<String>) {
        self.summary = summary;
    }

    fn set_location(&mut self, location: Option<String>) {
        self.location = location;
    }

    fn set_description(&mut self, desc: Option<String>) {
        self.desc = desc;
    }

    fn set_last_modified(&mut self, date: CalDate) {
        self.last_mod = Some(date);
    }

    fn set_stamp(&mut self, date: CalDate) {
        self.stamp = date;
    }

    fn set_rrule(&mut self, rrule: Option<CalRRule>) {
        self.rrule = rrule;
    }

    fn set_rid(&mut self, rid: Option<CalDate>) {
        self.rid = rid;
    }

    fn toggle_exclude(&mut self, date: CalDate) {
        if self.exdates.contains(&date) {
            self.exdates.retain(|d| d != &date);
        } else {
            self.exdates.push(date);
        }
    }

    fn set_alarms(&mut self, alarms: Option<Vec<CalAlarm>>) {
        self.alarms = alarms;
    }

    fn set_attendees(&mut self, attendees: Option<Vec<CalAttendee>>) {
        self.attendees = attendees;
    }

    fn set_organizer(&mut self, organizer: Option<CalOrganizer>) {
        self.organizer = organizer;
    }

    fn set_priority(&mut self, prio: Option<u8>) {
        self.priority = prio;
    }
}

/// The component type.
#[derive(Default, Copy, Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalCompType {
    /// Represents a VEVENT.
    #[default]
    Event,
    /// Represents a VTODO.
    Todo,
}

impl Display for CalCompType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// The type of component date.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CompDateType {
    /// Start of an event/TODO.
    Start,
    /// End of an event or due date of a TODO.
    EndOrDue,
}

/// Iterator for the dates of a specific component.
///
/// For non-recurrent components, it simply delivers the single date it occurs on. For recurrent
/// components, it delivers its occurrences, sorted by dates ascendingly. Typically, this iterator
/// is created by methods like [`CalComponent::dates_between`], which deliver all occurrences in
/// a certain time period.
#[derive(Default)]
pub struct CompDateIterator<'a> {
    recur: Option<RecurIterator<'a>>,
    exdates: Vec<DateTime<Tz>>,
    single: Option<(CompDateType, DateTime<Tz>)>,
}

impl<'a> CompDateIterator<'a> {
    fn new_recur(iter: RecurIterator<'a>, exdates: Vec<DateTime<Tz>>) -> Self {
        Self {
            recur: Some(iter),
            exdates,
            single: None,
        }
    }

    fn new_single(ty: CompDateType, single: DateTime<Tz>) -> Self {
        Self {
            recur: None,
            exdates: vec![],
            single: Some((ty, single)),
        }
    }
}

impl Iterator for CompDateIterator<'_> {
    type Item = (CompDateType, DateTime<Tz>, bool);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(recur) = &mut self.recur {
            recur
                .next()
                .map(|date| (CompDateType::Start, date, self.exdates.contains(&date)))
        } else {
            self.single.take().map(|(ty, date)| (ty, date, false))
        }
    }
}

/// Represents a component in an iCalendar object.
#[derive(Debug, Eq, PartialEq)]
pub enum CalComponent {
    /// A VEVENT component.
    Event(CalEvent),
    /// A VTODO component.
    Todo(CalTodo),
}

impl CalComponent {
    /// Returns the component as an [`CalEvent`], if it is an event.
    pub fn as_event(&self) -> Option<&CalEvent> {
        match self {
            Self::Event(ev) => Some(ev),
            _ => None,
        }
    }

    /// Returns the component as a mutable [`CalEvent`], if it is an event.
    pub fn as_event_mut(&mut self) -> Option<&mut CalEvent> {
        match self {
            Self::Event(ev) => Some(ev),
            _ => None,
        }
    }

    /// Returns the component as an [`CalTodo`], if it is a TODO.
    pub fn as_todo(&self) -> Option<&CalTodo> {
        match self {
            Self::Todo(todo) => Some(todo),
            _ => None,
        }
    }

    /// Returns the component as a mutable [`CalTodo`], if it is a TODO.
    pub fn as_todo_mut(&mut self) -> Option<&mut CalTodo> {
        match self {
            Self::Todo(todo) => Some(todo),
            _ => None,
        }
    }

    fn exdates_as_datetime(&self, tz: &Tz) -> Vec<DateTime<Tz>> {
        self.exdates()
            .iter()
            .map(|d| d.as_start_with_tz(tz))
            .collect::<Vec<_>>()
    }

    /// Returns an iterator with the occurrence dates in the given time period.
    ///
    /// For non-recurrent components, the occurrence is simply the start/end date when this
    /// component takes place. For recurrent components, there are potentially many occurrences.
    /// The iterator delivers the dates of these occurrences in the given time period. An
    /// occurrence is considered to be in this time period, if it overlaps with the period. That
    /// is, if either start or the end is in the period or the occurrence starts before and ends
    /// after the period.
    ///
    /// Note that the iterator returns excluded occurrences as well and requires the caller to
    /// ignore these, if desired.
    pub fn dates_between(&self, start: DateTime<Tz>, end: DateTime<Tz>) -> CompDateIterator<'_> {
        if let Some(rrule) = self.rrule() {
            let Some(dtstart) = self.start() else {
                return CompDateIterator::default();
            };

            let dtstart = dtstart.as_datetime(&start.timezone());
            let dates = rrule.dates_between(dtstart, self.time_duration(), start, end);
            let exdates = self.exdates_as_datetime(&start.timezone());
            return CompDateIterator::new_recur(dates, exdates);
        }

        if let Some(ev_start) = self.start() {
            let ev_start = ev_start.as_start_with_tz(&start.timezone());
            if ev_start > end {
                return CompDateIterator::default();
            }
        }

        if let Some(ev_end) = self.end_or_due() {
            let tzend = ev_end.as_end_with_tz(&start.timezone());
            if tzend < start {
                return CompDateIterator::default();
            }
        }

        match (self.start(), self.end_or_due()) {
            (Some(ev_start), _) => CompDateIterator::new_single(
                CompDateType::Start,
                ev_start.as_start_with_tz(&start.timezone()),
            ),
            (None, Some(ev_end)) => CompDateIterator::new_single(
                CompDateType::EndOrDue,
                ev_end.as_end_with_tz(&start.timezone()),
            ),
            (None, None) => CompDateIterator::default(),
        }
    }
}

impl PropertyProducer for CalComponent {
    fn to_props(&self) -> Vec<Property> {
        match self {
            Self::Event(ev) => ev.to_props(),
            Self::Todo(td) => td.to_props(),
        }
    }
}

macro_rules! get_with_ev_or_todo {
    ($self:tt, $method:tt) => {
        match $self {
            Self::Event(ev) => ev.inner.$method(),
            Self::Todo(td) => td.inner.$method(),
        }
    };
}

macro_rules! set_with_ev_or_todo {
    ($self:tt, $method:tt, $val:expr) => {
        match $self {
            Self::Event(ev) => ev.inner.$method($val),
            Self::Todo(td) => td.inner.$method($val),
        }
    };
}

impl EventLike for CalComponent {
    fn ctype(&self) -> CalCompType {
        match self {
            Self::Event(_) => CalCompType::Event,
            Self::Todo(_) => CalCompType::Todo,
        }
    }

    fn uid(&self) -> &String {
        get_with_ev_or_todo!(self, uid)
    }

    fn stamp(&self) -> &CalDate {
        get_with_ev_or_todo!(self, stamp)
    }

    fn created(&self) -> Option<&CalDate> {
        get_with_ev_or_todo!(self, created)
    }

    fn last_modified(&self) -> Option<&CalDate> {
        get_with_ev_or_todo!(self, last_modified)
    }

    fn start(&self) -> Option<&CalDate> {
        get_with_ev_or_todo!(self, start)
    }

    fn end_or_due(&self) -> Option<&CalDate> {
        match self {
            Self::Event(ev) => ev.end(),
            Self::Todo(td) => td.due(),
        }
    }

    fn duration(&self) -> Option<&CalDuration> {
        get_with_ev_or_todo!(self, duration)
    }

    fn summary(&self) -> Option<&String> {
        get_with_ev_or_todo!(self, summary)
    }

    fn description(&self) -> Option<&String> {
        get_with_ev_or_todo!(self, description)
    }

    fn location(&self) -> Option<&String> {
        get_with_ev_or_todo!(self, location)
    }

    fn categories(&self) -> Option<&[String]> {
        get_with_ev_or_todo!(self, categories)
    }

    fn organizer(&self) -> Option<&CalOrganizer> {
        get_with_ev_or_todo!(self, organizer)
    }

    fn attendees(&self) -> Option<&[CalAttendee]> {
        get_with_ev_or_todo!(self, attendees)
    }

    fn exdates(&self) -> &[CalDate] {
        get_with_ev_or_todo!(self, exdates)
    }

    fn alarms(&self) -> Option<&[CalAlarm]> {
        get_with_ev_or_todo!(self, alarms)
    }

    fn rrule(&self) -> Option<&CalRRule> {
        get_with_ev_or_todo!(self, rrule)
    }

    fn rid(&self) -> Option<&CalDate> {
        get_with_ev_or_todo!(self, rid)
    }

    fn priority(&self) -> Option<u8> {
        get_with_ev_or_todo!(self, priority)
    }
}

impl UpdatableEventLike for CalComponent {
    fn set_start(&mut self, start: Option<CalDate>) {
        set_with_ev_or_todo!(self, set_start, start);
    }

    fn set_summary(&mut self, summary: Option<String>) {
        set_with_ev_or_todo!(self, set_summary, summary);
    }

    fn set_location(&mut self, location: Option<String>) {
        set_with_ev_or_todo!(self, set_location, location);
    }

    fn set_description(&mut self, desc: Option<String>) {
        set_with_ev_or_todo!(self, set_description, desc);
    }

    fn set_last_modified(&mut self, date: CalDate) {
        set_with_ev_or_todo!(self, set_last_modified, date);
    }

    fn set_stamp(&mut self, date: CalDate) {
        set_with_ev_or_todo!(self, set_stamp, date);
    }

    fn set_rrule(&mut self, rrule: Option<CalRRule>) {
        set_with_ev_or_todo!(self, set_rrule, rrule);
    }

    fn set_rid(&mut self, rid: Option<CalDate>) {
        set_with_ev_or_todo!(self, set_rid, rid);
    }

    fn toggle_exclude(&mut self, date: CalDate) {
        set_with_ev_or_todo!(self, toggle_exclude, date);
    }

    fn set_alarms(&mut self, alarms: Option<Vec<CalAlarm>>) {
        set_with_ev_or_todo!(self, set_alarms, alarms);
    }

    fn set_attendees(&mut self, attendees: Option<Vec<CalAttendee>>) {
        set_with_ev_or_todo!(self, set_attendees, attendees);
    }

    fn set_organizer(&mut self, organizer: Option<CalOrganizer>) {
        set_with_ev_or_todo!(self, set_organizer, organizer);
    }

    fn set_priority(&mut self, prio: Option<u8>) {
        set_with_ev_or_todo!(self, set_priority, prio);
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use chrono_tz::UTC;

    use crate::objects::{CalComponent, CalEvent, Calendar, CompDateType};
    use crate::parser::{LineReader, ParseError, Property, PropertyProducer};

    use super::{CalCompType, EventLikeComponent};

    fn parse_prop_line(component: &mut EventLikeComponent, line: &str) -> Result<(), ParseError> {
        let mut lines = LineReader::new("".as_bytes());
        let prop = line.parse::<Property>().unwrap();
        component.parse_prop(&mut lines, prop)
    }

    #[test]
    fn parse_prop_round_trips_specific_values() {
        let mut comp = EventLikeComponent::new_empty(CalCompType::Event);

        for line in [
            "UID:uid-123",
            "CREATED:20250101T120000Z",
            "LAST-MODIFIED:20250101T121500Z",
            "DTSTAMP:20250101T123000Z",
            "DTSTART:20250102T090000Z",
            "DURATION:PT45M",
            "SUMMARY:Quarterly planning",
            "DESCRIPTION:Plan work and assign owners",
            "LOCATION:Conference Room A",
            "CATEGORIES:Engineering\\,Platform, Operations",
            "ORGANIZER;CN=Alex Lead:mailto:alex@example.com",
            "ATTENDEE;CN=Alice:mailto:alice@example.com",
            "ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED:mailto:alice@example.com",
            "EXDATE:20250103T090000Z,20250104T090000Z",
            "PRIORITY:3",
            "RRULE:FREQ=DAILY;COUNT=2",
            "RECURRENCE-ID:20250103T090000Z",
            "X-CUSTOM:kept-as-generic-prop",
        ] {
            parse_prop_line(&mut comp, line).unwrap();
        }

        let attendee_strings = comp
            .attendees
            .as_ref()
            .unwrap()
            .iter()
            .map(|attendee| attendee.to_prop().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            attendee_strings,
            vec![String::from(
                "ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CN=Alice:mailto:alice@example.com"
            )]
        );

        let prop_strings = comp
            .to_props()
            .into_iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            prop_strings,
            vec![
                String::from("UID:uid-123"),
                String::from("CREATED:20250101T120000Z"),
                String::from("LAST-MODIFIED:20250101T121500Z"),
                String::from("DTSTAMP:20250101T123000Z"),
                String::from("DTSTART:20250102T090000Z"),
                String::from("DURATION:PT45M"),
                String::from("SUMMARY:Quarterly planning"),
                String::from("DESCRIPTION:Plan work and assign owners"),
                String::from("LOCATION:Conference Room A"),
                String::from("CATEGORIES:Engineering\\,Platform,Operations"),
                String::from("ORGANIZER;CN=Alex Lead:mailto:alex@example.com"),
                String::from(
                    "ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CN=Alice:mailto:alice@example.com"
                ),
                String::from("EXDATE:20250103T090000Z"),
                String::from("EXDATE:20250104T090000Z"),
                String::from("PRIORITY:3"),
                String::from("RRULE:FREQ=DAILY;COUNT=2"),
                String::from("RECURRENCE-ID:20250103T090000Z"),
                String::from("X-CUSTOM:kept-as-generic-prop"),
            ]
        );
    }

    #[test]
    fn parse_prop_reports_specific_begin_and_alarm_drain_errors() {
        let mut comp = EventLikeComponent::new_empty(CalCompType::Event);

        let non_alarm_err = parse_prop_line(&mut comp, "BEGIN:VEVENT").unwrap_err();
        assert_eq!(
            non_alarm_err,
            ParseError::UnexpectedBegin(String::from("VEVENT"))
        );

        let mut wrong_end_lines = LineReader::new("TRIGGER:not-a-date\nEND:VEVENT\n".as_bytes());
        let wrong_end = comp
            .parse_prop(
                &mut wrong_end_lines,
                "BEGIN:VALARM".parse::<Property>().unwrap(),
            )
            .unwrap_err();
        assert_eq!(wrong_end, ParseError::UnexpectedEnd(String::from("VEVENT")));

        let mut eof_lines = LineReader::new("TRIGGER:not-a-date\n".as_bytes());
        let eof = comp
            .parse_prop(&mut eof_lines, "BEGIN:VALARM".parse::<Property>().unwrap())
            .unwrap_err();
        assert_eq!(eof, ParseError::UnexpectedEOF);
    }

    #[test]
    fn parse_prop_rejects_invalid_priority() {
        let mut comp = EventLikeComponent::new_empty(CalCompType::Event);
        let err = parse_prop_line(&mut comp, "PRIORITY:10").unwrap_err();
        assert_eq!(err, ParseError::InvalidPriority(10));
    }

    #[test]
    fn dates_between_handles_missing_and_due_only_dates() {
        let start = UTC.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let end = UTC.with_ymd_and_hms(2025, 1, 10, 0, 0, 0).unwrap();

        let missing_start: Calendar = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:r-1\nDTSTAMP:20250101T000000Z\nRRULE:FREQ=DAILY;COUNT=2\nEND:VEVENT\nEND:VCALENDAR"
            .parse()
            .unwrap();
        assert_eq!(
            missing_start.components()[0]
                .dates_between(start, end)
                .next(),
            None
        );

        let due_only: Calendar = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:t-1\nDTSTAMP:20250101T000000Z\nDUE:20250103T090000Z\nEND:VTODO\nEND:VCALENDAR"
            .parse()
            .unwrap();
        let mut due_only_dates = due_only.components()[0].dates_between(start, end);
        assert_eq!(
            due_only_dates.next(),
            Some((
                CompDateType::EndOrDue,
                UTC.with_ymd_and_hms(2025, 1, 3, 9, 0, 0).unwrap(),
                false,
            ))
        );
        assert_eq!(due_only_dates.next(), None);

        let due_before_range: Calendar = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:t-2\nDTSTAMP:20250101T000000Z\nDUE:20241231T230000Z\nEND:VTODO\nEND:VCALENDAR"
            .parse()
            .unwrap();
        assert_eq!(
            due_before_range.components()[0]
                .dates_between(start, end)
                .next(),
            None
        );

        let no_dates = CalComponent::Event(CalEvent::new("e-no-dates"));
        assert_eq!(no_dates.dates_between(start, end).next(), None);
    }

    #[test]
    fn component_type_display_is_exact() {
        assert_eq!(format!("{}", CalCompType::Event), "Event");
        assert_eq!(format!("{}", CalCompType::Todo), "Todo");
    }
}
