// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Context;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_ical::objects::{CalComponent, CalDate, CalEventStatus, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::Deserialize;

use crate::api::{JsonError, run_post};
use crate::util;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: String,
}

type Response = ();

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/cancel", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    run_post(state, move |state| Box::pin(run_cancel(state, req))).await
}

async fn run_cancel(
    state: &mut eventix_state::State,
    req: Request,
) -> anyhow::Result<Json<Response>> {
    let locale = state.locale();

    let user_mail = util::user_for_uid(state, &req.uid)?.map(|a| a.address());

    let rid = req
        .rid
        .parse::<CalDate>()
        .context(format!("Invalid rid date: '{}'", req.rid))?;

    let file = state
        .store_mut()
        .file_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    file.change_single(
        &req.uid,
        Some(&rid),
        locale.timezone(),
        user_mail.as_ref(),
        true,
        |base: Option<&CalComponent>, c: &mut CalComponent| {
            if c.as_event().unwrap().status() == Some(CalEventStatus::Cancelled) {
                return Err("Occurrence is already canceled".to_string());
            }
            let summary = match base {
                Some(base) => base.summary(),
                None => c.summary(),
            };
            if let Some(sum) = summary {
                c.set_summary(Some(format!("Canceled: {sum}")));
            }
            c.as_event_mut()
                .unwrap()
                .set_status(Some(CalEventStatus::Cancelled));
            c.touch();
            Ok(())
        },
    )?;

    file.save().context(format!(
        "Unable to save item with uid '{}' and rid '{:?}'",
        req.uid, req.rid
    ))?;

    Ok(Json(()))
}
