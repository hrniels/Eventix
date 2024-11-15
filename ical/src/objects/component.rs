use anyhow::anyhow;
use chrono::DateTime;
use chrono_tz::Tz;
use itertools::Itertools;

use crate::objects::{
    CalAttendee, CalDate, CalEvent, CalOrganizer, CalRRule, CalTodo, EventLike, UpdatableEventLike,
};
use crate::parser::{Property, PropertyProducer};

#[derive(Default, Debug, Eq, PartialEq)]
pub struct EventLikeComponent {
    uid: String,
    stamp: CalDate,
    created: Option<CalDate>,
    last_mod: Option<CalDate>,
    start: Option<CalDate>,
    summary: Option<String>,
    desc: Option<String>,
    location: Option<String>,
    categories: Option<Vec<String>>,
    organizer: Option<CalOrganizer>,
    attendees: Option<Vec<CalAttendee>>,
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

    pub fn set_start(&mut self, start: Option<CalDate>) {
        self.start = start;
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
                self.categories = Some(
                    prop.value()
                        .split(',')
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
                if !attendees.iter().any(|a| a.address() == att.address()) {
                    attendees.push(att);
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
        if let Some(ref cats) = self.categories {
            props.push(Property::new_escaped(
                "CATEGORIES",
                vec![],
                cats.iter().join(","),
            ));
        }
        if let Some(ref org) = self.organizer {
            props.push(org.to_prop());
        }
        if let Some(ref atts) = self.attendees {
            props.extend(atts.iter().map(|a| a.to_prop()));
        }
        if let Some(prio) = self.priority {
            props.push(Property::new("PRIORITY", vec![], format!("{}", prio)));
        }
        if let Some(rrule) = &self.rrule {
            props.push(Property::new_escaped("RRULE", vec![], format!("{}", rrule)));
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

    fn categories(&self) -> Option<&[String]> {
        self.categories.as_ref().map(|c| c.as_ref())
    }

    fn organizer(&self) -> Option<&CalOrganizer> {
        self.organizer.as_ref()
    }

    fn attendees(&self) -> Option<&[CalAttendee]> {
        self.attendees.as_ref().map(|a| a.as_ref())
    }

    fn rrule(&self) -> Option<&CalRRule> {
        self.rrule.as_ref()
    }

    fn rid(&self) -> Option<&CalDate> {
        self.rid.as_ref()
    }
}

impl UpdatableEventLike for EventLikeComponent {
    fn set_uid(&mut self, uid: String) {
        self.uid = uid;
    }

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
}

#[derive(Debug, Eq, PartialEq)]
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

    pub fn as_event_mut(&mut self) -> Option<&mut CalEvent> {
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

    pub fn as_todo_mut(&mut self) -> Option<&mut CalTodo> {
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

    fn rrule(&self) -> Option<&CalRRule> {
        get_with_ev_or_todo!(self, rrule)
    }

    fn rid(&self) -> Option<&CalDate> {
        get_with_ev_or_todo!(self, rid)
    }
}

impl UpdatableEventLike for CalComponent {
    fn set_uid(&mut self, uid: String) {
        set_with_ev_or_todo!(self, set_uid, uid);
    }

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
}
