use std::sync::Arc;

use askama::Template;

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
    pub fn new<N>(name: N, calendars: Calendars, calendar: Arc<String>) -> Self
    where
        N: ToString,
    {
        let name = name.to_string();
        Self {
            id: name.replace("[", "_").replace("]", "_"),
            name,
            calendars,
            calendar,
        }
    }
}
