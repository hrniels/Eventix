// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::{Json, extract::State};
use eventix_locale::Locale;
use eventix_state::EventixState;
use std::sync::Arc;

use crate::api::{HTMLResponse, JsonError};
use crate::comps::syncer::SyncerTemplate;
use crate::html::filters;

use crate::api::collections::Form;

#[derive(Template)]
#[template(path = "ajax/collections/add.htm")]
struct AddCollectionTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    name: String,
    syncer: SyncerTemplate<'a>,
    errors: Vec<String>,
}

pub async fn handler(State(state): State<EventixState>) -> Result<Json<HTMLResponse>, JsonError> {
    let locale = state.lock().await.locale();
    content_with(locale, State(state), Form::new(), Vec::new()).await
}

pub async fn content_with(
    locale: Arc<dyn Locale + Send + Sync>,
    State(_state): State<EventixState>,
    form: Form,
    errors: Vec<String>,
) -> Result<Json<HTMLResponse>, JsonError> {
    let html = AddCollectionTemplate {
        name: form.name.unwrap_or_default(),
        syncer: SyncerTemplate::new(locale.clone(), "syncer", form.syncer, None, false),
        locale,
        errors: errors.clone(),
    }
    .render()
    .context("add collection form template")?;

    Ok(Json(HTMLResponse::with_errors(html, errors)))
}

pub async fn save_handler(
    State(state): State<EventixState>,
    crate::extract::MultiForm(form): crate::extract::MultiForm<Form>,
) -> Result<Json<HTMLResponse>, JsonError> {
    let locale = state.lock().await.locale();
    let mut page = crate::pages::Page::default();

    let success = {
        let mut state = state.lock().await;
        if !form.check(&mut page, &locale, &state, false) {
            false
        } else {
            let syncer = form.syncer.to_syncer(None).await?;
            let col = eventix_state::CollectionSettings::new(syncer);
            state
                .settings_mut()
                .collections_mut()
                .insert(form.name.clone().unwrap(), col);

            if let Err(e) = state.settings().write_to_file() {
                tracing::warn!("Unable to save settings: {}", e);
                page.add_localized_error(&locale, &state, e);
                false
            } else {
                true
            }
        }
    };

    if success {
        Ok(Json(HTMLResponse::new(String::new())))
    } else {
        let errors = page.errors().to_vec();
        content_with(locale, State(state), form, errors).await
    }
}
