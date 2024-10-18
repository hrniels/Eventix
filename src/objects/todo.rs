use anyhow::anyhow;
use std::io::BufRead;

use crate::objects::{CalDate, RecurrenceRule, Status};
use crate::parser::{LineReader, Property, PropertyConsumer};

#[derive(Default, Debug)]
pub struct Todo {
    uid: String,
    created: CalDate,
    last_mod: CalDate,
    categories: Vec<String>,
    status: Option<Status>,
    completed: Option<CalDate>,
    summary: Option<String>,
    desc: Option<String>,
    start: Option<CalDate>,
    due: Option<CalDate>,
    rrule: Option<RecurrenceRule>,
    // 0 = undefined; 1 = highest, 9 = lowest
    priority: Option<u8>,
    percent: Option<u8>,
    props: Vec<Property>,
}

impl Todo {
    pub fn uid(&self) -> &String {
        &self.uid
    }

    pub fn start(&self) -> Option<&CalDate> {
        self.start.as_ref()
    }

    pub fn due(&self) -> Option<&CalDate> {
        self.due.as_ref()
    }

    pub fn rrule(&self) -> Option<&RecurrenceRule> {
        self.rrule.as_ref()
    }

    pub fn summary(&self) -> Option<&String> {
        self.summary.as_ref()
    }
}

impl PropertyConsumer for Todo {
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
                "END" => {
                    if prop.value() != "VTODO" {
                        return Err(anyhow!("Unexpected END:{}", prop.value()));
                    }
                    break Ok(comp);
                }
                "UID" => {
                    comp.uid = prop.take_value();
                }
                "CREATED" => {
                    comp.created = prop.try_into()?;
                }
                "SUMMARY" => {
                    comp.summary = Some(prop.take_value());
                }
                "DTSTART" => {
                    comp.start = Some(prop.try_into()?);
                }
                "DUE" => {
                    comp.due = Some(prop.try_into()?);
                }
                "RRULE" => {
                    comp.rrule = Some(prop.value().parse()?);
                }
                _ => {
                    comp.props.push(prop);
                }
            }
        }
    }
}

// impl TryFrom<&IcalTodo> for Todo {
//     type Error = anyhow::Error;
//
//     fn try_from(value: &IcalTodo) -> Result<Self, Self::Error> {
//         let mut todo = Todo::default();
//
//         let Some(uid) = value.get_property("UID") else {
//             return Err(anyhow!("UID property missing"));
//         };
//         todo.uid = uid
//             .value
//             .as_ref()
//             .ok_or_else(|| anyhow!("UID property value missing"))?
//             .clone();
//
//         if let Some(stamp) = value.get_property("DTSTAMP") {
//             let stamp_date: ICalDate = stamp.try_into()?;
//             todo.created = stamp_date.clone();
//             todo.last_mod = stamp_date;
//         }
//
//         if let Some(date) = value.get_property("CREATED") {
//             todo.created = date.try_into()?;
//         }
//         if let Some(date) = value.get_property("LAST-MODIFIED") {
//             todo.last_mod = date.try_into()?;
//         }
//         if let Some(date) = value.get_property("COMPLETED") {
//             todo.completed = Some(date.try_into()?);
//         }
//
//         if let Some(cats) = value.get_property("CATEGORIES") {
//             if let Some(cats) = cats.value.as_ref() {
//                 todo.categories = cats.split(',').map(|v| v.trim().to_string()).collect();
//             }
//         }
//         if let Some(summary) = value.get_property("SUMMARY") {
//             if let Some(summary) = summary.value.as_ref() {
//                 todo.summary = Some(summary.clone());
//             }
//         }
//         if let Some(desc) = value.get_property("DESCRIPTION") {
//             if let Some(desc) = desc.value.as_ref() {
//                 todo.desc = Some(desc.clone());
//             }
//         }
//
//         if let Some(date) = value.get_property("DTSTART") {
//             todo.start = Some(date.try_into()?);
//         }
//         if let Some(date) = value.get_property("DUE") {
//             todo.due = Some(date.try_into()?);
//         }
//
//         if let Some(status) = value.get_property("STATUS") {
//             if let Some(status) = status.value.as_ref() {
//                 todo.status = Some(status.parse()?);
//             }
//         }
//
//         if let Some(prio) = value.get_property("PRIORITY") {
//             if let Some(prio) = prio.value.as_ref() {
//                 let prio = prio.parse()?;
//                 if prio >= 10 {
//                     return Err(anyhow!("Invalid priority: {}", prio));
//                 }
//                 todo.priority = Some(prio);
//             }
//         }
//         if let Some(percent) = value.get_property("PERCENT") {
//             if let Some(percent) = percent.value.as_ref() {
//                 let percent = percent.parse()?;
//                 if percent > 100 {
//                     return Err(anyhow!("Invalid percent: {}", percent));
//                 }
//                 todo.percent = Some(percent);
//             }
//         }
//
//         Ok(todo)
//     }
// }
//
// #[cfg(test)]
// mod tests {
//     use chrono::{Local, NaiveDate, TimeZone, Utc};
//     use ical::IcalParser;
//
//     use crate::objects::{date::ICalDate, todo::ICalStatus};
//
//     use super::Todo;
//
//     #[test]
//     fn basics() {
//         let todo_str = "
// BEGIN:VCALENDAR
// BEGIN:VTODO
// UID:20070313T123432Z-456553@example.com
// DTSTAMP:20070313T123432Z
// DUE;VALUE=DATE:20070501
// SUMMARY:Submit Quebec Income Tax Return for 2006
// CLASS:CONFIDENTIAL
// CATEGORIES:FAMILY,FINANCE
// STATUS:NEEDS-ACTION
// END:VTODO
// END:VCALENDAR
//         ";
//         let mut reader = IcalParser::new(todo_str.as_bytes());
//         let cal = reader.next().unwrap().unwrap();
//         let todo = &cal.todos[0];
//         let todo: Todo = todo.try_into().unwrap();
//
//         assert_eq!(&todo.uid, "20070313T123432Z-456553@example.com");
//         assert_eq!(
//             todo.summary,
//             Some("Submit Quebec Income Tax Return for 2006".to_string())
//         );
//
//         let stamp = ICalDate::DateTimeUtc(
//             Utc.with_ymd_and_hms(2007, 3, 13, 12, 34, 32)
//                 .unwrap()
//                 .with_timezone(&Local),
//         );
//         assert_eq!(todo.created, stamp);
//         assert_eq!(todo.last_mod, stamp);
//
//         assert_eq!(
//             todo.due,
//             Some(ICalDate::Date(NaiveDate::from_ymd_opt(2007, 5, 1).unwrap()))
//         );
//
//         assert_eq!(todo.status, Some(ICalStatus::NeedsAction));
//         assert_eq!(
//             todo.categories,
//             vec!["FAMILY".to_string(), "FINANCE".to_string()]
//         );
//     }
// }
