// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::mem::discriminant;

use anyhow::{Context, Result, anyhow};
use askama::Template;
use axum::{Json, extract::State};
use eventix_locale::Locale;
use eventix_state::EventixState;
use std::sync::Arc;

use crate::api::{HTMLResponse, JsonError};
use crate::comps::syncer::SyncerTemplate;
use crate::html::filters;

use crate::api::collections::{Form, Request};

#[derive(Template)]
#[template(path = "ajax/collections/edit.htm")]
struct EditCollectionTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    col_id: String,
    syncer: SyncerTemplate<'a>,
    errors: Vec<String>,
}

pub async fn handler(
    State(state): State<EventixState>,
    axum::extract::Query(req): axum::extract::Query<Request>,
) -> Result<Json<HTMLResponse>, JsonError> {
    let locale = state.lock().await.locale();
    content_with(locale, State(state), req, None, Vec::new()).await
}

pub async fn content_with(
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<EventixState>,
    req: Request,
    form: Option<Form>,
    errors: Vec<String>,
) -> Result<Json<HTMLResponse>, JsonError> {
    let col = {
        let state = state.lock().await;
        state
            .settings()
            .collections()
            .get(&req.col_id)
            .ok_or_else(|| anyhow!("No collection '{}'", req.col_id))?
            .clone()
    };

    let form = if let Some(form) = form {
        form
    } else {
        Form::new_from(&col)
    };

    let syncer = form.syncer_type();

    let html = EditCollectionTemplate {
        col_id: req.col_id,
        syncer: SyncerTemplate::new(locale.clone(), "syncer", form.syncer, syncer, true),
        locale,
        errors: errors.clone(),
    }
    .render()
    .context("edit collection form template")?;

    Ok(Json(HTMLResponse::with_errors(html, errors)))
}

pub async fn save_handler(
    State(state): State<EventixState>,
    axum::extract::Query(req): axum::extract::Query<Request>,
    crate::extract::MultiForm(form): crate::extract::MultiForm<Form>,
) -> Result<Json<HTMLResponse>, JsonError> {
    let locale = state.lock().await.locale();
    let mut page = crate::pages::Page::default();

    let success = {
        let mut state_guard = state.lock().await;
        if !form.check(&mut page, &locale, &state_guard, true) {
            false
        } else {
            let cols = state_guard.settings_mut().collections_mut();
            let col = match cols.get_mut(&req.col_id) {
                Some(col) => col,
                None => {
                    return Err(anyhow!("No collection '{}'", req.col_id).into());
                }
            };

            match form.syncer.to_syncer(Some(col.syncer())).await {
                Ok(syncer) => {
                    if discriminant(&syncer) != discriminant(col.syncer()) {
                        page.add_error(locale.translate("error.syncer_change"));
                        false
                    } else {
                        col.set_syncer(syncer);

                        if let Err(e) = state_guard.settings().write_to_file() {
                            tracing::warn!("Unable to save settings: {}", e);
                            page.add_localized_error(&locale, &state_guard, e);
                            false
                        } else {
                            true
                        }
                    }
                }
                Err(e) => {
                    page.add_localized_error(&locale, &state_guard, e);
                    false
                }
            }
        }
    };

    if success {
        Ok(Json(HTMLResponse::new(String::new())))
    } else {
        let errors = page.errors().to_vec();
        content_with(
            locale,
            State(state),
            Request { col_id: req.col_id },
            Some(form),
            errors,
        )
        .await
    }
}
