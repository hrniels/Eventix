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
        let syncer = form.syncer.to_syncer(None).await?;
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

    let errors = {
        let mut state = state.lock().await;
        match action_update(&mut page, &locale, &mut state, &mut form).await {
            Ok(true) => {
                page.add_info(locale.translate("info.collection_added"));

                form = Form::new();
                Vec::new()
            }
            Ok(false) => page.errors().to_vec(),
            Err(e) => {
                page.add_localized_error(&locale, &state, e);
                page.errors().to_vec()
            }
        }
    };

    super::index::content_with(page, locale, State(state), form, req, errors).await
}
