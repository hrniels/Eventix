use askama::Template;
use ical::col::{CalSource, Id};
use std::sync::Arc;

use crate::locale::Locale;

#[derive(Template)]
#[template(path = "comps/calcombo.htm")]
pub struct CalComboTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    name: String,
    id: String,
    calendars: &'a [CalSource],
    calendar: Id,
}

impl<'a> CalComboTemplate<'a> {
    pub fn new<N: ToString>(
        locale: Arc<dyn Locale + Send + Sync>,
        name: N,
        calendars: &'a [CalSource],
        calendar: Id,
    ) -> Self {
        let name = name.to_string();
        Self {
            locale,
            id: name.replace("[", "_").replace("]", "_"),
            name,
            calendars,
            calendar,
        }
    }
}
