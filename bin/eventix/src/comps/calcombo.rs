use askama::Template;
use ical::col::{CalSource, Id};

#[derive(Template)]
#[template(path = "comps/calcombo.htm")]
pub struct CalComboTemplate<'a> {
    name: String,
    id: String,
    calendars: &'a [CalSource],
    calendar: Id,
}

impl<'a> CalComboTemplate<'a> {
    pub fn new<N: ToString>(name: N, calendars: &'a [CalSource], calendar: Id) -> Self {
        let name = name.to_string();
        Self {
            id: name.replace("[", "_").replace("]", "_"),
            name,
            calendars,
            calendar,
        }
    }
}
