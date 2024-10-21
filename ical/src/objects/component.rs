use chrono::DateTime;
use chrono_tz::Tz;
use std::ops::Deref;

use crate::objects::{CalDate, CalEvent, CalTodo, EventLike};

#[derive(Debug)]
pub enum CalComponent {
    Event(CalEvent),
    Todo(CalTodo),
}

impl Deref for CalComponent {
    type Target = EventLike;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Event(ev) => ev.inner(),
            Self::Todo(td) => td.inner(),
        }
    }
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

    pub fn end_or_due(&self) -> Option<&CalDate> {
        match self {
            Self::Event(ev) => ev.end(),
            Self::Todo(td) => td.due(),
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
