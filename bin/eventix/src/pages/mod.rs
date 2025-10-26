pub mod calendars;
pub mod edit;
pub mod error;
pub mod list;
pub mod monthly;
pub mod new;
pub mod weekly;

mod events;
mod tasks;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use eventix_locale::Locale;
use eventix_state::EventixState;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::objects::{Calendar, Calendars};

pub struct Page {
    start: Instant,
    now: DateTime<Tz>,
    errors: Vec<String>,
    infos: Vec<String>,
    calendars: Calendars,
    last_reload: NaiveDateTime,
    debug: bool,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            now: Local::now().with_timezone(&Tz::UTC),
            errors: Vec::new(),
            infos: Vec::new(),
            calendars: Calendars::default(),
            last_reload: NaiveDateTime::default(),
            debug: cfg!(debug_assertions),
        }
    }
}

impl Page {
    pub async fn new(state: &EventixState) -> Self {
        let state = state.lock().await;
        let locale = state.settings().locale();
        Self {
            start: Instant::now(),
            now: Local::now().with_timezone(locale.timezone()),
            calendars: Calendars::new(&state, |_settings| true),
            last_reload: state.last_reload(),
            ..Default::default()
        }
    }

    pub fn debug(&self) -> bool {
        self.debug
    }

    pub fn git_hash(&self) -> &str {
        &env!("GIT_HASH")[..7]
    }

    pub fn last_reload(&self, locale: &Arc<dyn Locale + Send + Sync>) -> DateTime<Tz> {
        locale.timezone().from_utc_datetime(&self.last_reload)
    }

    pub fn calendars(&self) -> &[Calendar] {
        &self.calendars.0
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
