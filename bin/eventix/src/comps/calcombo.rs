use std::sync::Arc;

use askama::Template;
use ical::col::CalDir;

use crate::objects::Calendars;

#[derive(Template)]
#[template(path = "comps/calcombo.htm")]
pub struct CalComboTemplate {
    name: String,
    id: String,
    calendars: Calendars,
    calendar: Arc<String>,
}

impl CalComboTemplate {
    pub fn new<'a, N, I>(name: N, calendars: I, calendar: Arc<String>) -> Self
    where
        I: Iterator<Item = &'a CalDir>,
        N: ToString,
    {
        let name = name.to_string();
        Self {
            id: name.replace("[", "_").replace("]", "_"),
            name,
            calendars: Calendars::new(calendars),
            calendar,
        }
    }
}
