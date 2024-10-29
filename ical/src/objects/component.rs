use anyhow::anyhow;
use chrono::DateTime;
use chrono_tz::Tz;
use itertools::Itertools;

use crate::objects::{CalAttendee, CalDate, CalEvent, CalRRule, CalTodo, EventLike};
use crate::parser::{Property, PropertyProducer};

#[derive(Default, Debug)]
pub struct EventLikeComponent {
    uid: String,
    stamp: CalDate,
    created: Option<CalDate>,
    last_mod: Option<CalDate>,
    start: Option<CalDate>,
    summary: Option<String>,
    desc: Option<String>,
    location: Option<String>,
    categories: Vec<String>,
    attendees: Vec<CalAttendee>,
    // 0 = undefined; 1 = highest, 9 = lowest
    priority: Option<u8>,
    rrule: Option<CalRRule>,
    rid: Option<CalDate>,
    props: Vec<Property>,
}

impl EventLikeComponent {
    pub fn set_uid<T: ToString>(&mut self, uid: T) {
        self.uid = uid.to_string();
    }

    pub fn set_start(&mut self, start: CalDate) {
        self.start = Some(start);
    }

    pub(crate) fn parse_prop(&mut self, prop: Property) -> Result<(), anyhow::Error> {
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
                self.categories = prop
                    .value()
                    .split(',')
                    .map(|v| v.trim().to_string())
                    .collect();
            }
            "ATTENDEE" => {
                let att: CalAttendee = prop.try_into()?;
                if !self.attendees.iter().any(|a| a.address() == att.address()) {
                    self.attendees.push(att);
                }
            }
            "PRIORITY" => {
                let prio = prop.value().parse()?;
                if prio >= 10 {
                    return Err(anyhow!("Invalid priority: {}", prio));
                }
                self.priority = Some(prio);
            }
            "RRULE" => {
                self.rrule = Some(prop.value().parse()?);
            }
            "RECURRENCE-ID" => {
                self.rid = Some(prop.try_into()?);
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
        if let Some(ref summary) = self.summary {
            props.push(Property::new("SUMMARY", vec![], summary.clone()));
        }
        if let Some(ref desc) = self.desc {
            props.push(Property::new("DESCRIPTION", vec![], desc.clone()));
        }
        if let Some(ref loc) = self.location {
            props.push(Property::new("LOCATION", vec![], loc.clone()));
        }
        if !self.categories.is_empty() {
            props.push(Property::new_escaped(
                "CATEGORIES",
                vec![],
                self.categories.iter().join(","),
            ));
        }
        if !self.attendees.is_empty() {
            props.extend(self.attendees.iter().map(|a| a.to_prop()));
        }
        if let Some(prio) = self.priority {
            props.push(Property::new("PRIORITY", vec![], format!("{}", prio)));
        }
        if let Some(rrule) = &self.rrule {
            props.push(Property::new("RRULE", vec![], format!("{}", rrule)));
        }
        if let Some(ref rid) = self.rid {
            props.push(rid.to_prop("RECURRENCE-ID"));
        }
        props.extend(self.props.iter().cloned());
        props
    }
}

impl EventLike for EventLikeComponent {
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

    fn summary(&self) -> Option<&String> {
        self.summary.as_ref()
    }

    fn description(&self) -> Option<&String> {
        self.desc.as_ref()
    }

    fn location(&self) -> Option<&String> {
        self.location.as_ref()
    }

    fn categories(&self) -> &[String] {
        self.categories.as_ref()
    }

    fn attendees(&self) -> &[CalAttendee] {
        self.attendees.as_ref()
    }

    fn rrule(&self) -> Option<&CalRRule> {
        self.rrule.as_ref()
    }

    fn rid(&self) -> Option<&CalDate> {
        self.rid.as_ref()
    }
}

#[derive(Debug)]
pub enum CalComponent {
    Event(CalEvent),
    Todo(CalTodo),
}

impl CalComponent {
    pub fn is_event(&self) -> bool {
        matches!(self, Self::Event(_))
    }

    pub fn is_todo(&self) -> bool {
        matches!(self, Self::Todo(_))
    }

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

    pub fn dates_within(&self, start: DateTime<Tz>, end: DateTime<Tz>) -> Vec<DateTime<Tz>> {
        if let Some(rrule) = self.rrule() {
            let Some(dtstart) = self.start() else {
                return vec![];
            };
            return rrule.dates_within(dtstart.as_start_with_tz(&start.timezone()), start, end);
        }

        let Some(ev_start) = self.start_or_created() else {
            return vec![];
        };
        let ev_start = ev_start.as_start_with_tz(&start.timezone());
        if ev_start > end {
            return vec![];
        }
        if let Some(ev_end) = self.end_or_due() {
            let tzend = ev_end.as_end_with_tz(&start.timezone());
            if tzend < start {
                return vec![];
            }
        }

        vec![ev_start]
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

macro_rules! with_ev_or_todo {
    ($self:tt, $method:tt) => {
        match $self {
            Self::Event(ev) => ev.inner.$method(),
            Self::Todo(td) => td.inner.$method(),
        }
    };
}

impl EventLike for CalComponent {
    fn uid(&self) -> &String {
        with_ev_or_todo!(self, uid)
    }

    fn stamp(&self) -> &CalDate {
        with_ev_or_todo!(self, stamp)
    }

    fn created(&self) -> Option<&CalDate> {
        with_ev_or_todo!(self, created)
    }

    fn last_modified(&self) -> Option<&CalDate> {
        with_ev_or_todo!(self, last_modified)
    }

    fn start(&self) -> Option<&CalDate> {
        with_ev_or_todo!(self, start)
    }

    fn end_or_due(&self) -> Option<&CalDate> {
        match self {
            Self::Event(ev) => ev.end(),
            Self::Todo(td) => td.due(),
        }
    }

    fn summary(&self) -> Option<&String> {
        with_ev_or_todo!(self, summary)
    }

    fn description(&self) -> Option<&String> {
        with_ev_or_todo!(self, description)
    }

    fn location(&self) -> Option<&String> {
        with_ev_or_todo!(self, location)
    }

    fn categories(&self) -> &[String] {
        with_ev_or_todo!(self, categories)
    }

    fn attendees(&self) -> &[CalAttendee] {
        with_ev_or_todo!(self, attendees)
    }

    fn rrule(&self) -> Option<&CalRRule> {
        with_ev_or_todo!(self, rrule)
    }

    fn rid(&self) -> Option<&CalDate> {
        with_ev_or_todo!(self, rid)
    }
}
