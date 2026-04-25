// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_ical::objects::{CalDate, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::api::{JsonError, run_post};
use crate::util;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
    rid: CalDate,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/toggle", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(form): Query<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    run_post(state, move |state| Box::pin(run_toggle(state, form))).await
}

async fn run_toggle(
    state: &mut eventix_state::State,
    form: Request,
) -> anyhow::Result<Json<Response>> {
    let user_mail = util::user_for_uid(state, &form.uid)?.map(|a| a.address());

    let file = state
        .store_mut()
        .try_file_by_id_mut(&form.uid)
        .context(format!("Unable to find component with uid '{}'", form.uid))?;

    let base = file
        .component_with_mut(|c| c.rid().is_none() && c.uid() == &form.uid)
        .ok_or_else(|| anyhow!("Unable to find base component with uid {}", form.uid))?;

    if !base.is_owned_by(user_mail.as_ref()) {
        return Err(anyhow!("No edit permission"));
    }

    let exdate = form.rid.normalize_to(base.start().unwrap());
    base.toggle_exclude(exdate);
    base.set_last_modified(CalDate::now());
    base.set_stamp(CalDate::now());
    file.save()
        .with_context(|| format!("Unable to save item with uid {}", form.uid))?;

    Ok(Json(Response {}))
}
