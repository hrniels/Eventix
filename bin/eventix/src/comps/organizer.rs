use askama::Template;
use eventix_ical::objects::CalOrganizer;
use eventix_locale::Locale;
use std::sync::Arc;

use crate::html::filters;

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
