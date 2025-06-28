use askama::Template;
use ical::objects::{CalAttendee, CalRole};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, sync::Arc};

use crate::html::filters;
use crate::locale::Locale;

#[derive(Default, Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AttendeeRole {
    #[default]
    Required,
    Optional,
}

impl fmt::Display for AttendeeRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl From<CalRole> for AttendeeRole {
    fn from(role: CalRole) -> Self {
        match role {
            CalRole::Optional => Self::Optional,
            _ => Self::Required,
        }
    }
}

impl From<AttendeeRole> for CalRole {
    fn from(role: AttendeeRole) -> Self {
        match role {
            AttendeeRole::Required => CalRole::Required,
            AttendeeRole::Optional => CalRole::Optional,
        }
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Attendees {
    #[serde(rename = "name")]
    names: Vec<String>,
    #[serde(rename = "role")]
    roles: Vec<AttendeeRole>,
}

impl Attendees {
    pub fn new_from_cal_attendees(att: Option<&[CalAttendee]>) -> Self {
        match att {
            Some(atts) => {
                let mut attendees = Self {
                    names: Vec::new(),
                    roles: Vec::new(),
                };
                for att in atts {
                    attendees.names.push(att.pretty_name());
                    attendees
                        .roles
                        .push(att.role().unwrap_or(CalRole::Required).into());
                }
                attendees
            }
            None => Self::default(),
        }
    }

    pub fn to_cal_attendees(&self) -> Option<Vec<CalAttendee>> {
        if self.names.is_empty() || self.names.len() != self.roles.len() {
            return None;
        }

        let addr_name_regex = Regex::new(r"([^<]*?)\s+<(.*)>").unwrap();

        let mut atts = Vec::new();
        for (name, role) in self.names.iter().zip(&self.roles) {
            let mut att = if let Some(matches) = addr_name_regex.captures(name) {
                let addr = matches.get(2).unwrap().as_str();
                let name = matches.get(1).unwrap().as_str();
                let mut att = CalAttendee::new(format!("mailto:{addr}"));
                att.set_common_name(name.to_string());
                att
            } else {
                CalAttendee::new(format!("mailto:{name}"))
            };
            att.set_role((*role).into());
            atts.push(att);
        }
        Some(atts)
    }
}

#[derive(Template)]
#[template(path = "comps/attendees.htm")]
pub struct AttendeesTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
    name: String,
    id: String,
    emails: HashMap<String, String>,
    cal_combo_id: Option<String>,
    attendees: Attendees,
}

impl AttendeesTemplate {
    pub fn new<N: ToString>(
        locale: Arc<dyn Locale + Send + Sync>,
        name: N,
        emails: HashMap<String, String>,
        cal_combo_id: Option<String>,
        attendees: Option<Attendees>,
    ) -> Self {
        let name = name.to_string();
        Self {
            locale,
            id: name.replace("[", "_").replace("]", "_"),
            name,
            emails,
            cal_combo_id,
            attendees: attendees.unwrap_or_default(),
        }
    }
}
