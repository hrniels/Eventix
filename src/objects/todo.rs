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
                "LAST-MODIFIED" => {
                    comp.last_mod = prop.try_into()?;
                }
                "COMPLETED" => {
                    comp.completed = Some(prop.try_into()?);
                }
                "DTSTAMP" => {
                    let stamp_date: CalDate = prop.try_into()?;
                    comp.created = stamp_date.clone();
                    comp.last_mod = stamp_date.clone();
                }
                "SUMMARY" => {
                    comp.summary = Some(prop.take_value());
                }
                "DESCRIPTION" => {
                    comp.desc = Some(prop.take_value());
                }
                "CATEGORIES" => {
                    comp.categories = prop
                        .value()
                        .split(',')
                        .map(|v| v.trim().to_string())
                        .collect();
                }
                "DTSTART" => {
                    comp.start = Some(prop.try_into()?);
                }
                "DUE" => {
                    comp.due = Some(prop.try_into()?);
                }
                "STATUS" => {
                    comp.status = Some(prop.value().parse()?);
                }
                "RRULE" => {
                    comp.rrule = Some(prop.value().parse()?);
                }
                "PRIORITY" => {
                    let prio = prop.value().parse()?;
                    if prio >= 10 {
                        return Err(anyhow!("Invalid priority: {}", prio));
                    }
                    comp.priority = Some(prio);
                }
                "PERCENT-COMPLETE" => {
                    let percent = prop.value().parse()?;
                    if percent > 100 {
                        return Err(anyhow!("Invalid percent: {}", percent));
                    }
                    comp.percent = Some(percent);
                }
                _ => {
                    comp.props.push(prop);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};

    use crate::objects::{calendar::Calendar, date::CalDateTime, CalDate, Status};

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
        let todo = &cal.components()[0].as_todo().unwrap();

        assert_eq!(&todo.uid, "20070313T123432Z-456553@example.com");
        assert_eq!(
            todo.summary,
            Some("Submit Quebec Income Tax Return for 2006".to_string())
        );

        let stamp = CalDate::DateTime(CalDateTime::Utc(
            Utc.with_ymd_and_hms(2007, 3, 13, 12, 34, 32).unwrap(),
        ));
        assert_eq!(todo.created, stamp);
        assert_eq!(todo.last_mod, stamp);

        assert_eq!(
            todo.due,
            Some(CalDate::Date(NaiveDate::from_ymd_opt(2007, 5, 1).unwrap()))
        );

        assert_eq!(todo.status, Some(Status::NeedsAction));
        assert_eq!(
            todo.categories,
            vec!["FAMILY".to_string(), "FINANCE".to_string()]
        );

        assert_eq!(todo.percent, Some(10));
    }
}
