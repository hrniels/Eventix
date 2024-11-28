use std::sync::Arc;

use askama::Template;
use ical::col::CalSource;

#[derive(Template)]
#[template(path = "comps/calcombo.htm")]
pub struct CalComboTemplate<'a> {
    name: String,
    id: String,
    calendars: &'a [CalSource],
    calendar: Arc<String>,
}

impl<'a> CalComboTemplate<'a> {
    pub fn new<N: ToString>(name: N, calendars: &'a [CalSource], calendar: Arc<String>) -> Self {
        let name = name.to_string();
        Self {
            id: name.replace("[", "_").replace("]", "_"),
            name,
            calendars,
            calendar,
        }
    }
}
