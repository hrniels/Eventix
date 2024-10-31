use std::{ops::Deref, sync::Mutex};

use ical::col::Occurrence;
use ical::objects::EventLike;
use once_cell::sync::Lazy;

pub struct DayOccurrence<'a> {
    id: u64,
    inner: &'a Occurrence<'a>,
}

impl<'a> DayOccurrence<'a> {
    pub fn new(inner: &'a Occurrence<'a>) -> Self {
        static NEXT_ID: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(0));
        let mut next = NEXT_ID.lock().unwrap();
        let id = *next + 1;
        *next += 1;
        Self { id, inner }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn uid_html(&self) -> String {
        self.inner
            .uid()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect()
    }

    pub fn rid_html(&self) -> String {
        match self.inner.rid() {
            Some(rid) => rid.to_string(),
            None => self
                .inner
                .occurrence_start()
                .to_utc()
                .format("%Y%m%dT%H%M%SZ")
                .to_string(),
        }
    }

    pub fn status_class(&self) -> Option<String> {
        self.inner.event_status().map(|st| format!("{:?}", st))
    }
}

impl<'a> Deref for DayOccurrence<'a> {
    type Target = Occurrence<'a>;
    fn deref(&self) -> &Self::Target {
        self.inner
    }
}
