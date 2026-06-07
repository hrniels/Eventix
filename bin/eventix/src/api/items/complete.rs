// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Context;
use axum::{
    Json, Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::post,
};
use eventix_ical::objects::{
    CalComponent, CalDate, CalTodoStatus, PRIORITY_MEDIUM, UpdatableEventLike,
};
use eventix_state::EventixState;
use serde::Deserialize;

use crate::{
    api::{JsonError, run_post},
    util,
};

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<CalDate>,
}

type Response = ();

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/complete", post(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, JsonError> {
    run_post(state, move |state| Box::pin(run_complete(state, req))).await
}

async fn run_complete(
    state: &mut eventix_state::State,
    req: Request,
) -> anyhow::Result<Json<Response>> {
    let locale = state.locale();

    let user_mail = util::user_for_uid(state, &req.uid)?.map(|a| a.address());

    let file = state
        .store_mut()
        .file_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    file.change_single(
        &req.uid,
        req.rid.as_ref(),
        locale.timezone(),
        user_mail.as_ref(),
        true,
        |_base, c: &mut CalComponent| {
            let td = c.as_todo_mut().unwrap();
            td.set_status(Some(CalTodoStatus::Completed));
            td.set_percent(Some(100));
            td.set_completed(Some(CalDate::now()));
            // set the priority as is required by MS exchange as soon as TODOs are completed - unsure
            // why; we don't care about the priority at the moment and thus are fine with any value.
            td.set_priority(Some(PRIORITY_MEDIUM));
            td.touch();
            Ok(())
        },
    )?;

    file.save().context(format!(
        "Unable to save item with uid '{}' and rid '{:?}'",
        req.uid, req.rid
    ))?;

    Ok(Json(()))
}
