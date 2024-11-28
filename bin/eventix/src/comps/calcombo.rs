use std::sync::Arc;

use askama::Template;
use ical::col::CalSource;

struct Source {
    id: Arc<String>,
    name: String,
}

#[derive(Template)]
#[template(path = "comps/calcombo.htm")]
pub struct CalComboTemplate {
    name: String,
    id: String,
    calendars: Vec<Source>,
    calendar: Arc<String>,
}

impl CalComboTemplate {
    pub fn new<N: ToString>(name: N, calendars: &[CalSource], calendar: Arc<String>) -> Self {
        let mut calendars = calendars
            .iter()
            .map(|s| Source {
                id: s.id().clone(),
                name: s.name().clone(),
            })
            .collect::<Vec<_>>();
        calendars.sort_by(|a, b| a.name.cmp(&b.name));

        let name = name.to_string();
        Self {
            id: name.replace("[", "_").replace("]", "_"),
            name,
            calendars,
            calendar,
        }
    }
}
