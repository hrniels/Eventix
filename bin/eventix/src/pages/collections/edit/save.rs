// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::anyhow;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use eventix_locale::Locale;
use eventix_state::EventixState;
use std::mem::discriminant;
use std::sync::Arc;

use crate::extract::MultiForm;
use crate::pages::collections::Form;
use crate::pages::{Page, error::HTMLError};

use super::Request;

async fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    state: &mut eventix_state::State,
    form: &mut Form,
    req: &Request,
) -> anyhow::Result<bool> {
    if !form.check(page, locale, state, true) {
        return Ok(false);
    }

    let cols = state.settings_mut().collections_mut();
    let col = cols
        .get_mut(&req.col_id)
        .ok_or_else(|| anyhow!("No collection {}", req.col_id))?;

    let syncer = form.syncer.to_syncer().unwrap();
    if discriminant(&syncer) != discriminant(col.syncer()) {
        page.add_error(locale.translate("error.syncer_change"));
        return Ok(false);
    }

    col.set_syncer(syncer);

    if let Err(e) = state.settings().write_to_file() {
        tracing::warn!("Unable to save settings: {}", e);
        return Err(e);
    }

    Ok(true)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
    MultiForm(mut form): MultiForm<Form>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let mut page = super::new_page(&state).await;

    let (locale, form) = {
        let mut state = state.lock().await;
        let locale = state.locale();
        let form = if action_update(&mut page, &locale, &mut state, &mut form, &req).await? {
            page.add_info(locale.translate("info.collection_updated"));
            None
        } else {
            Some(form)
        };
        (locale, form)
    };

    super::index::content_with(page, locale, State(state), form, req).await
}
