use anyhow::anyhow;
use chrono::DateTime;
use chrono_tz::Tz;

use crate::objects::{CalDate, CalEvent, CalRRule, CalTodo, EventLike};
use crate::parser::Property;

#[derive(Default, Debug)]
pub struct EventLikeComponent {
    uid: String,
    created: CalDate,
    last_mod: CalDate,
    start: Option<CalDate>,
    summary: Option<String>,
    desc: Option<String>,
    location: Option<String>,
    categories: Vec<String>,
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
                self.created = prop.try_into()?;
            }
            "LAST-MODIFIED" => {
                self.last_mod = prop.try_into()?;
            }
            "DTSTAMP" => {
                let stamp_date: CalDate = prop.try_into()?;
                self.created = stamp_date.clone();
                self.last_mod = stamp_date.clone();
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

impl EventLike for EventLikeComponent {
    fn uid(&self) -> &String {
        &self.uid
    }

    fn created(&self) -> &CalDate {
        &self.created
    }

    fn last_modified(&self) -> &CalDate {
        &self.last_mod
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

        match (self.start(), self.end_or_due()) {
            (Some(ev_start), Some(ev_end)) => {
                let tzstart = ev_start.as_start_with_tz(&start.timezone());
                let tzend = ev_end.as_end_with_tz(&start.timezone());
                if tzstart > end || tzend < start {
                    vec![]
                } else {
                    vec![tzstart]
                }
            }
            (Some(ev_start), None) => {
                let tzstart = ev_start.as_start_with_tz(&start.timezone());
                if tzstart > end {
                    vec![]
                } else {
                    vec![end]
                }
            }
            (None, Some(ev_end)) => {
                let tzend = ev_end.as_end_with_tz(&start.timezone());
                if tzend < start {
                    vec![]
                } else {
                    vec![start]
                }
            }
            (None, None) => vec![start],
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

    fn created(&self) -> &CalDate {
        with_ev_or_todo!(self, created)
    }

    fn last_modified(&self) -> &CalDate {
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

    fn rrule(&self) -> Option<&CalRRule> {
        with_ev_or_todo!(self, rrule)
    }

    fn rid(&self) -> Option<&CalDate> {
        with_ev_or_todo!(self, rid)
    }
}
