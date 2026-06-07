// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use axum::{
    Json, Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::post,
};
use eventix_ical::objects::{CalComponent, CalDate, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::Deserialize;

use crate::api::{JsonError, run_post};

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<CalDate>,
    delay_days: u32,
}

type Response = ();

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/postpone", post(handler))
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
    if req.delay_days > 7 {
        return Err(anyhow!("Unsupported number of days"));
    }

    let locale = state.locale();

    let file = state
        .store_mut()
        .file_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    let complete = |c: &mut CalComponent| -> anyhow::Result<()> {
        let td = c.as_todo_mut().unwrap();
        let Some(due) = td.due() else {
            return Err(anyhow!("TODO has no due date"));
        };
        td.set_due(Some(due.add_days(req.delay_days)));
        td.touch();
        Ok(())
    };

    if let Some(comp) =
        file.component_with_mut(|c| c.uid() == &req.uid && c.rid() == req.rid.as_ref())
    {
        complete(comp)?;
    } else {
        let comp = file.component_with(|c| c.uid() == &req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component '{}' is not recurrent", req.uid));
        }

        file.create_overwrite(
            &req.uid,
            req.rid.clone().unwrap(),
            locale.timezone(),
            |_base, comp| complete(comp),
        )
        .context("Creating overwrite failed")?;
    }
    file.save().context(format!(
        "Unable to save item with uid '{}' and rid '{:?}'",
        req.uid, req.rid
    ))?;

    Ok(Json(()))
}
