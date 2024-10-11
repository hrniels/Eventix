use std::str::FromStr;

use anyhow::anyhow;
use ical::parser::{ical::component::IcalTodo, Component};

use super::date::ICalDate;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum TodoStatus {
    NeedsAction,
    Completed,
    InProcess,
    Cancelled,
}

impl FromStr for TodoStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "NEEDS-ACTION" => Ok(Self::NeedsAction),
            "COMPLETED" => Ok(Self::Completed),
            "IN-PROCESS" => Ok(Self::InProcess),
            "CANCELLED" => Ok(Self::Cancelled),
            _ => Err(anyhow!("Invalid status {}", s)),
        }
    }
}

#[derive(Default)]
pub struct Todo {
    uid: String,
    created: ICalDate,
    last_mod: ICalDate,
    categories: Vec<String>,
    status: Option<TodoStatus>,
    completed: Option<ICalDate>,
    summary: Option<String>,
    desc: Option<String>,
    start: Option<ICalDate>,
    due: Option<ICalDate>,
    // 0 = undefined; 1 = highest, 9 = lowest
    priority: Option<u8>,
    percent: Option<u8>,
}

impl TryFrom<&IcalTodo> for Todo {
    type Error = anyhow::Error;

    fn try_from(value: &IcalTodo) -> Result<Self, Self::Error> {
        let mut todo = Todo::default();

        let Some(uid) = value.get_property("UID") else {
            return Err(anyhow!("UID property missing"));
        };
        todo.uid = uid
            .value
            .as_ref()
            .ok_or_else(|| anyhow!("UID property value missing"))?
            .clone();

        let Some(stamp) = value.get_property("DTSTAMP") else {
            return Err(anyhow!("DTSTAMP property missing"));
        };
        let stamp_date: ICalDate = stamp.try_into()?;
        todo.created = stamp_date.clone();
        todo.last_mod = stamp_date;

        if let Some(date) = value.get_property("CREATED") {
            todo.created = date.try_into()?;
        }
        if let Some(date) = value.get_property("LAST-MODIFIED") {
            todo.last_mod = date.try_into()?;
        }
        if let Some(date) = value.get_property("COMPLETED") {
            todo.completed = Some(date.try_into()?);
        }

        if let Some(cats) = value.get_property("CATEGORIES") {
            if let Some(cats) = cats.value.as_ref() {
                todo.categories = cats.split(',').map(|v| v.trim().to_string()).collect();
            }
        }
        if let Some(summary) = value.get_property("SUMMARY") {
            if let Some(summary) = summary.value.as_ref() {
                todo.summary = Some(summary.clone());
            }
        }
        if let Some(desc) = value.get_property("DESCRIPTION") {
            if let Some(desc) = desc.value.as_ref() {
                todo.desc = Some(desc.clone());
            }
        }

        if let Some(date) = value.get_property("DTSTART") {
            todo.start = Some(date.try_into()?);
        }
        if let Some(date) = value.get_property("DUE") {
            todo.due = Some(date.try_into()?);
        }

        if let Some(status) = value.get_property("STATUS") {
            if let Some(status) = status.value.as_ref() {
                todo.status = Some(status.parse()?);
            }
        }

        if let Some(prio) = value.get_property("PRIORITY") {
            if let Some(prio) = prio.value.as_ref() {
                let prio = prio.parse()?;
                if prio >= 10 {
                    return Err(anyhow!("Invalid priority: {}", prio));
                }
                todo.priority = Some(prio);
            }
        }
        if let Some(percent) = value.get_property("PERCENT") {
            if let Some(percent) = percent.value.as_ref() {
                let percent = percent.parse()?;
                if percent > 100 {
                    return Err(anyhow!("Invalid percent: {}", percent));
                }
                todo.percent = Some(percent);
            }
        }

        Ok(todo)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Local, NaiveDate, TimeZone, Utc};
    use ical::IcalParser;

    use crate::objects::{date::ICalDate, todo::TodoStatus};

    use super::Todo;

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
END:VTODO
END:VCALENDAR
        ";
        let mut reader = IcalParser::new(todo_str.as_bytes());
        let cal = reader.next().unwrap().unwrap();
        let todo = &cal.todos[0];
        let todo: Todo = todo.try_into().unwrap();

        assert_eq!(&todo.uid, "20070313T123432Z-456553@example.com");
        assert_eq!(
            todo.summary,
            Some("Submit Quebec Income Tax Return for 2006".to_string())
        );

        let stamp = ICalDate::DateTimeUtc(
            Utc.with_ymd_and_hms(2007, 3, 13, 12, 34, 32)
                .unwrap()
                .with_timezone(&Local),
        );
        assert_eq!(todo.created, stamp);
        assert_eq!(todo.last_mod, stamp);

        assert_eq!(
            todo.due,
            Some(ICalDate::Date(NaiveDate::from_ymd_opt(2007, 5, 1).unwrap()))
        );

        assert_eq!(todo.status, Some(TodoStatus::NeedsAction));
        assert_eq!(
            todo.categories,
            vec!["FAMILY".to_string(), "FINANCE".to_string()]
        );
    }
}
