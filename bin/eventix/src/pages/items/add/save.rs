// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use eventix_ical::objects::UpdatableEventLike;
use eventix_locale::Locale;
use eventix_state::EventixState;
use std::sync::Arc;

use crate::extract::MultiForm;
use crate::objects::{CompAction, create_component};
use crate::pages::{Page, error::HTMLError};

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

    let rrule = match form.rrule.to_rrule() {
        Ok(rrule) => rrule,
        Err(e) => {
            page.add_error(e);
            return Ok(false);
        }
    };

    create_component(
        state,
        locale,
        &form.calendar,
        req.ctype,
        |cal, alarm_type, comp, persalarms, organizer, ctx, locale| {
            comp.set_rrule(rrule);
            form.update(cal, alarm_type, comp, persalarms, organizer, ctx, locale)
        },
    )?;

    Ok(true)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
    MultiForm(mut form): MultiForm<CompNew>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = state.lock().await.locale();
    let mut page = super::new_page(&state).await;

    {
        let mut state = state.lock().await;
        match action_update(&mut page, &locale, &mut state, &mut form, &req).await {
            Ok(true) => {
                page.add_info(locale.translate("info.event_added"));

                form = CompNew::new(&req, locale.timezone(), Some(form.calendar));
            }
            Ok(false) => {}
            Err(e) => page.add_localized_error(&locale, &state, e),
        }
    }

    super::index::content_with(page, locale, State(state), form, req).await
}
