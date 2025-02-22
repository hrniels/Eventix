pub mod edit;
pub mod list;
pub mod monthly;
pub mod new;
pub mod weekly;

mod events;
mod tasks;

use std::time::{Duration, Instant};

use crate::{
    objects::{Calendar, Calendars},
    state::State,
};

#[derive(Debug, Clone)]
pub struct Breadcrumb {
    pub url: String,
    pub name: String,
}

impl Breadcrumb {
    #[allow(dead_code)]
    pub fn new<U: ToString, N: ToString>(url: U, name: N) -> Self {
        Self {
            url: url.to_string(),
            name: name.to_string(),
        }
    }
}

pub struct Page {
    start: Instant,
    breadcrumbs: Vec<Breadcrumb>,
    errors: Vec<String>,
    infos: Vec<String>,
    calendars: Calendars,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            breadcrumbs: Vec::new(),
            errors: Vec::new(),
            infos: Vec::new(),
            calendars: Calendars::default(),
        }
    }
}

impl Page {
    pub async fn new(state: &State) -> Self {
        Self {
            start: Instant::now(),
            calendars: Calendars::new_with_disabled(state).await,
            ..Default::default()
        }
    }

    pub fn calendars(&self) -> &[Calendar] {
        &self.calendars.0
    }

    pub fn breadcrumbs(&self) -> &[Breadcrumb] {
        &self.breadcrumbs
    }

    pub fn add_breadcrumb(&mut self, breadcrumb: Breadcrumb) {
        self.breadcrumbs.push(breadcrumb);
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    pub fn add_error<S: ToString>(&mut self, message: S) {
        self.errors.push(message.to_string());
    }

    #[allow(dead_code)]
    pub fn add_detailed_error(&mut self, error: anyhow::Error) {
        let mut msg = error.to_string();
        for m in error.chain().skip(1) {
            msg.push_str(": ");
            msg.push_str(&m.to_string());
        }
        self.add_error(msg);
    }

    pub fn infos(&self) -> &[String] {
        &self.infos
    }

    #[allow(dead_code)]
    pub fn add_info<S: ToString>(&mut self, message: S) {
        self.infos.push(message.to_string());
    }

    pub fn time_elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}
