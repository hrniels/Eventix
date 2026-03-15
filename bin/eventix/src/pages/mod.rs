// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod calendars;
pub mod callist;
pub mod collections;
pub mod error;
pub mod items;
pub mod list;
pub mod monthly;
pub mod shell;
pub mod sidebar;
pub mod weekly;

mod events;
mod tasks;

use axum::Router;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use eventix_ical::objects::CalCompType;
use eventix_locale::Locale;
use eventix_state::EventixState;
use std::sync::Arc;

use crate::{
    comps::calcombo::CalComboTemplate,
    objects::{Calendar, Calendars},
};

pub fn router(state: EventixState) -> Router {
    Router::new()
        .nest("/calendars", calendars::router(state.clone()))
        .nest("/callist", callist::router(state.clone()))
        .nest("/collections", collections::router(state.clone()))
        .nest("/items", items::router(state.clone()))
        .nest("/list", list::router(state.clone()))
        .nest("/monthly", monthly::router(state.clone()))
        .nest("/sidebar", sidebar::router(state.clone()))
        .nest("/weekly", weekly::router(state.clone()))
}

pub struct Page {
    now: DateTime<Tz>,
    errors: Vec<String>,
    infos: Vec<String>,
    calendars: Calendars,
    quickcals: Option<CalComboTemplate>,
    last_reload: NaiveDateTime,
    debug: bool,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            now: Local::now().with_timezone(&Tz::UTC),
            errors: Vec::new(),
            infos: Vec::new(),
            calendars: Calendars::default(),
            quickcals: None,
            last_reload: NaiveDateTime::default(),
            debug: cfg!(debug_assertions),
        }
    }
}

impl Page {
    pub async fn new(state: &EventixState) -> Self {
        let state = state.lock().await;
        let locale = state.locale();

        let calendar = Arc::new(match state.misc().last_calendar(CalCompType::Todo) {
            Some(cal) => cal.clone(),
            None => String::new(),
        });
        let calendars = Calendars::new(&state, |_id, settings| {
            settings.types().contains(&CalCompType::Todo)
        });

        Self {
            now: Local::now().with_timezone(locale.timezone()),
            calendars: Calendars::new(&state, |_id, _settings| true),
            quickcals: if !calendars.0.is_empty() {
                Some(CalComboTemplate::new(
                    "quicktodo_calendar",
                    calendars,
                    calendar,
                    true,
                ))
            } else {
                None
            },
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
}
