use std::io::BufRead;
use std::ops::{Deref, DerefMut};

use crate::objects::{CalDate, CalTodoStatus, EventLikeComponent};
use crate::parser::{LineReader, ParseError, Property, PropertyConsumer, PropertyProducer};

/// Represents an iCalendar TODO.
///
/// Each TODO has a unique id (uid) and several optional properties such as a summary, a
/// description, or alarms. A TODO shares many properties with
/// [`CalEvent`](crate::objects::CalEvent), which are implemented in [`EventLikeComponent`]. In
/// contrast to events, TODOs have a [`CalTodoStatus`] and a due date instead of an end date.
/// Furthermore, a TODO stores the progress in case the status is
/// [`InProcess`](CalTodoStatus::InProcess`) or when it was completed if the status is
/// [`Completed`](`CalTodoStatus::Completed`).
///
/// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.6.2>.
#[derive(Debug, Eq, PartialEq)]
pub struct CalTodo {
    pub(crate) inner: EventLikeComponent,
    due: Option<CalDate>,
    status: Option<CalTodoStatus>,
    completed: Option<CalDate>,
    percent: Option<u8>,
}

impl CalTodo {
    fn new_empty() -> Self {
        Self {
            inner: EventLikeComponent::new_empty(),
            due: None,
            status: None,
            completed: None,
            percent: None,
        }
    }

    /// Creates a new TODO with given uid.
    pub fn new<T: ToString>(uid: T) -> Self {
        let mut new = Self::new_empty();
        new.inner = EventLikeComponent::new(uid);
        new
    }

    /// Returns the due date of the TODO (DUE).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.2.3>.
    pub fn due(&self) -> Option<&CalDate> {
        self.due.as_ref()
    }

    /// Sets the due date for this TODO (DUE).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.2.3>.
    pub fn set_due(&mut self, due: Option<CalDate>) {
        self.due = due;
    }

    /// Returns the status of the TODO (STATUS).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.11>.
    pub fn status(&self) -> Option<CalTodoStatus> {
        self.status
    }

    /// Sets the status of this TODO (STATUS).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.11>.
    pub fn set_status(&mut self, status: Option<CalTodoStatus>) {
        self.status = status;
    }

    /// Returns the date when this TODO was completed (COMPLETE).
    ///
    /// TODOs only have a completed date if the status is [`Completed`](CalTodoStatus::Completed).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.2.1>.
    pub fn completed(&self) -> Option<&CalDate> {
        self.completed.as_ref()
    }

    /// Sets the completion date of this TODO (COMPLETE).
    ///
    /// Note that TODOs should only have a completed date if the status is
    /// [`Completed`](CalTodoStatus::Completed).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.2.1>.
    pub fn set_completed(&mut self, completed: Option<CalDate>) {
        self.completed = completed;
    }

    /// Returns the percentage of completion (PERCENT-COMPLETE).
    ///
    /// TODOs only have a percentage of completion if the status is
    /// [`InProcess`](CalTodoStatus::InProcess).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.8>.
    pub fn percent(&self) -> Option<u8> {
        self.percent
    }

    /// Sets the percentage of completion (PERCENT-COMPLETE).
    ///
    /// Note that TODOs should only have a percentage of completion if the status is
    /// [`InProcess`](CalTodoStatus::InProcess).
    ///
    /// See <https://datatracker.ietf.org/doc/html/rfc5545#section-3.8.1.8>.
    pub fn set_percent(&mut self, percent: Option<u8>) {
        self.percent = percent;
    }
}

impl Deref for CalTodo {
    type Target = EventLikeComponent;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for CalTodo {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl PropertyProducer for CalTodo {
    fn to_props(&self) -> Vec<Property> {
        let mut props = vec![Property::new("BEGIN", vec![], "VTODO")];
        if let Some(ref due) = self.due {
            props.push(due.to_prop("DUE"));
        }
        if let Some(status) = self.status {
            props.push(Property::new("STATUS", vec![], format!("{}", status)));
        }
        if let Some(ref completed) = self.completed {
            props.push(completed.to_prop("COMPLETED"));
        }
        if let Some(percent) = self.percent {
            props.push(Property::new(
                "PERCENT-COMPLETE",
                vec![],
                format!("{}", percent),
            ));
        }
        props.extend(self.inner.to_props());
        props.push(Property::new("END", vec![], "VTODO"));
        props
    }
}

impl PropertyConsumer for CalTodo {
    fn from_lines<R: BufRead>(
        lines: &mut LineReader<R>,
        _prop: Property,
    ) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        let mut comp = Self::new_empty();
        loop {
            let Some(line) = lines.next() else {
                break Err(ParseError::UnexpectedEOF);
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                "END" => {
                    if prop.value() != "VTODO" {
                        return Err(ParseError::UnexpectedEnd(prop.take_value()));
                    }
                    break Ok(comp);
                }
                "COMPLETED" => {
                    comp.completed = Some(prop.try_into()?);
                }
                "DUE" => {
                    comp.due = Some(prop.try_into()?);
                }
                "STATUS" => {
                    comp.status = Some(prop.value().parse()?);
                }
                "PERCENT-COMPLETE" => {
                    let percent = prop.value().parse()?;
                    if percent > 100 {
                        return Err(ParseError::InvalidPercent(percent));
                    }
                    comp.percent = Some(percent);
                }
                _ => {
                    comp.inner.parse_prop(lines, prop)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};

    use crate::objects::{CalDate, CalDateTime, CalDateType, CalTodoStatus, Calendar, EventLike};

    #[test]
    fn basics() {
        let todo_str = "
BEGIN:VCALENDAR
BEGIN:VTODO
UID:20070313T123432Z-456553@example.com
DTSTAMP:20070313T123432Z
DUE;VALUE=DATE:20070501
SUMMARY:Submit Quebec Income Tax Return for 2006
CLASS:CONFIDENTIAL
CATEGORIES:FAMILY,FINANCE
STATUS:NEEDS-ACTION
PERCENT-COMPLETE:10
END:VTODO
END:VCALENDAR";
        let cal = todo_str.parse::<Calendar>().unwrap();
        let todo = cal.components()[0].as_todo().unwrap();

        assert_eq!(todo.uid(), "20070313T123432Z-456553@example.com");
        assert_eq!(
            todo.summary(),
            Some(&"Submit Quebec Income Tax Return for 2006".to_string())
        );

        let stamp = CalDate::DateTime(CalDateTime::Utc(
            Utc.with_ymd_and_hms(2007, 3, 13, 12, 34, 32).unwrap(),
        ));
        assert_eq!(todo.stamp(), &stamp);

        assert_eq!(
            todo.due,
            Some(CalDate::Date(
                NaiveDate::from_ymd_opt(2007, 5, 1).unwrap(),
                CalDateType::Inclusive
            ))
        );

        assert_eq!(todo.status, Some(CalTodoStatus::NeedsAction));
        assert_eq!(
            todo.categories(),
            Some(vec!["FAMILY".to_string(), "FINANCE".to_string()].as_ref())
        );

        assert_eq!(todo.percent, Some(10));
    }
}
