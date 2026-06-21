// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod add;
pub mod delete;
pub mod edit;
pub mod log;

use axum::Router;
use eventix_locale::Locale;
use eventix_state::EventixState;
use serde::Deserialize;
use std::sync::Arc;

use eventix_state::CollectionSettings;

use crate::comps::syncer::{Syncer, SyncerRequest};
use crate::pages::Page;

#[derive(Default, Debug, Deserialize)]
pub struct Request {
    pub col_id: String,
}

#[derive(Default, Debug, Deserialize)]
pub struct Form {
    name: Option<String>,
    syncer: SyncerRequest,
}

impl Form {
    pub fn new() -> Self {
        Form {
            syncer: SyncerRequest::new(),
            ..Default::default()
        }
    }

    pub fn new_from(col: &CollectionSettings) -> Self {
        Self {
            name: None,
            syncer: SyncerRequest::new_from_syncer(col.syncer()),
        }
    }

    pub fn syncer_type(&self) -> Option<Syncer> {
        self.syncer.syncer()
    }

    pub fn check(
        &self,
        page: &mut Page,
        locale: &Arc<dyn Locale + Send + Sync>,
        state: &eventix_state::State,
        edit: bool,
    ) -> bool {
        if !edit {
            let name = self.name.clone().unwrap_or_default();
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                page.add_error(locale.translate("error.collection_name_chars"));
                return false;
            }

            if state.settings().collections().contains_key(&name) {
                page.add_error(locale.translate("error.collection_exists"));
                return false;
            }
        }

        self.syncer.check(page, locale, !edit)
    }
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .with_state(state.clone())
        .nest("/add", add::router(state.clone()))
        .merge(delete::router(state.clone()))
        .nest("/edit", edit::router(state.clone()))
        .merge(log::router(state.clone()))
}
