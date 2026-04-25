// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use axum::{
    Json, Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::post,
};
use eventix_ical::objects::{
    CalComponent, CalDate, CalTodoStatus, EventLike, PRIORITY_MEDIUM, UpdatableEventLike,
};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::api::{JsonError, run_post};

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<CalDate>,
}

#[derive(Debug, Serialize)]
struct Response {}

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

    let file = state
        .store_mut()
        .try_files_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    let complete = |c: &mut CalComponent| -> anyhow::Result<()> {
        let td = c.as_todo_mut().unwrap();
        td.set_status(Some(CalTodoStatus::Completed));
        td.set_percent(Some(100));
        td.set_completed(Some(CalDate::now()));
        // set the priority as is required by MS exchange as soon as TODOs are completed - unsure
        // why; we don't care about the priority at the moment and thus are fine with any value.
        td.set_priority(Some(PRIORITY_MEDIUM));
        td.set_last_modified(CalDate::now());
        td.set_stamp(CalDate::now());
        Ok(())
    };

    if let Some(comp) =
        file.component_with_mut(|c| c.uid() == &req.uid && c.rid() == req.rid.as_ref())
    {
        complete(comp)?;
    } else {
        let comp = file.component_with(|c| c.uid() == &req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", req.uid));
        }

        file.create_overwrite(
            &req.uid,
            req.rid.clone().unwrap(),
            locale.timezone(),
            |_base, comp| complete(comp),
        )
        .context("Creating overwrite failed")?;
    }
    file.save()
        .context(format!("Save file {}:{:?}", req.uid, req.rid))?;

    Ok(Json(Response {}))
}
