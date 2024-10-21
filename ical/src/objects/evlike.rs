use anyhow::anyhow;

use crate::objects::{CalDate, CalRRule};
use crate::parser::Property;

#[derive(Default, Debug)]
pub struct EventLike {
    uid: String,
    created: CalDate,
    last_mod: CalDate,
    start: Option<CalDate>,
    summary: Option<String>,
    desc: Option<String>,
    categories: Vec<String>,
    // 0 = undefined; 1 = highest, 9 = lowest
    priority: Option<u8>,
    rrule: Option<CalRRule>,
    rid: Option<CalDate>,
    props: Vec<Property>,
}

impl EventLike {
    pub fn uid(&self) -> &String {
        &self.uid
    }

    pub fn set_uid<T: ToString>(&mut self, uid: T) {
        self.uid = uid.to_string();
    }

    pub fn created(&self) -> &CalDate {
        &self.created
    }

    pub fn last_modified(&self) -> &CalDate {
        &self.last_mod
    }

    pub fn is_all_day(&self) -> bool {
        matches!(self.start, Some(CalDate::Date(_)))
    }

    pub fn start(&self) -> Option<&CalDate> {
        self.start.as_ref()
    }

    pub fn set_start(&mut self, start: CalDate) {
        self.start = Some(start);
    }

    pub fn summary(&self) -> Option<&String> {
        self.summary.as_ref()
    }

    pub fn description(&self) -> Option<&String> {
        self.desc.as_ref()
    }

    pub fn categories(&self) -> &[String] {
        &self.categories
    }

    pub fn rrule(&self) -> Option<&CalRRule> {
        self.rrule.as_ref()
    }

    pub fn rid(&self) -> Option<&CalDate> {
        self.rid.as_ref()
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
