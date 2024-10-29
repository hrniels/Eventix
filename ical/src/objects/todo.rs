use anyhow::anyhow;
use std::io::BufRead;
use std::ops::{Deref, DerefMut};

use crate::objects::{CalDate, CalTodoStatus, EventLikeComponent, Other};
use crate::parser::{LineReader, Property, PropertyConsumer, PropertyProducer};

#[derive(Default, Debug)]
pub struct CalTodo {
    pub(crate) inner: EventLikeComponent,
    due: Option<CalDate>,
    status: Option<CalTodoStatus>,
    completed: Option<CalDate>,
    percent: Option<u8>,
    other: Vec<Other>,
}

impl CalTodo {
    pub fn due(&self) -> Option<&CalDate> {
        self.due.as_ref()
    }

    pub fn status(&self) -> Option<CalTodoStatus> {
        self.status
    }

    pub fn completed(&self) -> Option<&CalDate> {
        self.completed.as_ref()
    }

    pub fn percent(&self) -> Option<u8> {
        self.percent
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
        props.extend(self.inner.to_props().into_iter());
        for o in &self.other {
            props.extend(o.to_props().into_iter());
        }
        props.push(Property::new("END", vec![], "VTODO"));
        props
    }
}

impl PropertyConsumer for CalTodo {
    fn from_lines<R: BufRead>(
        lines: &mut LineReader<R>,
        _prop: Property,
    ) -> Result<Self, anyhow::Error>
    where
        Self: Sized,
    {
        let mut comp = Self::default();
        loop {
            let Some(line) = lines.next() else {
                break Err(anyhow!("Unexpected EOF"));
            };

            let prop = line.parse::<Property>()?;
            match prop.name().as_str() {
                // TODO properly support alarms
                "BEGIN" => {
                    let other = Other::from_lines(lines, prop)?;
                    comp.other.push(other);
                }
                "END" => {
                    if prop.value() != "VTODO" {
                        return Err(anyhow!("Unexpected END:{}", prop.value()));
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
                        return Err(anyhow!("Invalid percent: {}", percent));
                    }
                    comp.percent = Some(percent);
                }
                _ => {
                    comp.inner.parse_prop(prop)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};

    use crate::objects::{CalDate, CalDateTime, CalTodoStatus, Calendar, EventLike};

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
            Some(CalDate::Date(NaiveDate::from_ymd_opt(2007, 5, 1).unwrap()))
        );

        assert_eq!(todo.status, Some(CalTodoStatus::NeedsAction));
        assert_eq!(
            todo.categories(),
            vec!["FAMILY".to_string(), "FINANCE".to_string()]
        );

        assert_eq!(todo.percent, Some(10));
    }
}
