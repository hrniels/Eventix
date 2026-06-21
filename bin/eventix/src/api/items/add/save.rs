// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{
    Json,
    extract::{Query, State},
};
use eventix_ical::objects::UpdatableEventLike;
use eventix_locale::Locale;
use eventix_state::EventixState;
use std::sync::Arc;

use crate::extract::MultiForm;
use crate::objects::{CompAction, create_component};
use crate::{
    api::{HTMLResponse, JsonError},
    pages::Page,
};

use super::{CompNew, Request};

async fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    state: &mut eventix_state::State,
    form: &mut CompNew,
    req: &Request,
) -> anyhow::Result<bool> {
    if !form.check(page, locale, req.ctype) {
        return Ok(false);
    }

    let start = form.start_end().as_caldates(locale, req.ctype.into()).0;
    let rrule = match form.rrule.to_rrule(start.as_ref()) {
        Ok(rrule) => rrule,
        Err(e) => {
            return Err(e);
        }
    };

    create_component(
        state,
        locale,
        &form.calendar,
        req.ctype,
        |cal, alarm_type, comp, persalarms, organizer, _ctx, locale| {
            comp.set_rrule(rrule);
            form.update(cal, alarm_type, comp, persalarms, organizer, locale)
        },
    )?;

    Ok(true)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
    MultiForm(mut form): MultiForm<CompNew>,
) -> anyhow::Result<Json<HTMLResponse>, JsonError> {
    let locale = state.lock().await.locale();

    let errors = {
        let mut page = Page::default();
        let mut state = state.lock().await;
        match action_update(&mut page, &locale, &mut state, &mut form, &req).await {
            Ok(true) => {
                page.add_info(locale.translate("info.event_added"));

                return Ok(Json(HTMLResponse::new(String::new())));
            }
            Ok(false) => page.errors().to_vec(),
            Err(e) => {
                page.add_localized_error(&locale, &state, e);
                page.errors().to_vec()
            }
        }
    };

    super::index::content_with(locale, State(state), form, req, errors).await
}
