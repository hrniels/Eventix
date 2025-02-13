use std::sync::Arc;

use askama::Template;
use ical::objects::CalOrganizer;

use crate::html::filters;
use crate::locale::Locale;

#[derive(Template)]
#[template(path = "comps/organizer.htm")]
pub struct OrganizerTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    org: &'a CalOrganizer,
}

impl<'a> OrganizerTemplate<'a> {
    pub fn new(locale: Arc<dyn Locale + Send + Sync>, org: &'a CalOrganizer) -> Self {
        Self { locale, org }
    }
}
