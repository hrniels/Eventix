// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use eventix_locale::Locale;
use eventix_state::{CollectionSettings, EventixState};
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
) -> anyhow::Result<bool> {
    if !form.check(page, locale, state, false) {
        return Ok(false);
    }

    {
        let cols = state.settings_mut().collections_mut();
        let syncer = form.syncer.to_syncer().unwrap();
        let col = CollectionSettings::new(syncer);
        cols.insert(form.name.clone().unwrap(), col);
    }

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
    let locale = state.lock().await.locale();
    let mut page = super::new_page(&state).await;

    {
        let mut state = state.lock().await;
        if action_update(&mut page, &locale, &mut state, &mut form).await? {
            page.add_info(locale.translate("info.collection_added"));

            form = Form::new();
        }
    }

    super::index::content(page, locale, State(state), form, req).await
}
