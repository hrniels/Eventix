use askama::Template;
use eventix_ical::objects::CalOrganizer;
use std::sync::Arc;

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
