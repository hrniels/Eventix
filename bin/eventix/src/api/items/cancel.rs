// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_ical::objects::{CalComponent, CalDate, CalEventStatus, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::api::{JsonError, run_post};
use crate::util;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: String,
}

#[derive(Debug, Serialize)]
struct Response {}

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
        .context(format!("Invalid rid date: {}", req.rid))?;

    let file = state
        .store_mut()
        .try_file_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    let checks = |c: &CalComponent| -> anyhow::Result<()> {
        if c.as_event().unwrap().status() == Some(CalEventStatus::Cancelled) {
            return Err(anyhow!("Occurrence is already canceled"));
        }
        if !c.is_owned_by(user_mail.as_ref()) {
            return Err(anyhow!("No edit permission"));
        }
        Ok(())
    };

    let complete = |base: Option<&CalComponent>, c: &mut CalComponent| -> anyhow::Result<()> {
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
        c.set_last_modified(CalDate::now());
        c.set_stamp(CalDate::now());
        Ok(())
    };

    if let Some(comp) = file.component_with_mut(|c| c.uid() == &req.uid && c.rid() == Some(&rid)) {
        checks(comp)?;
        complete(None, comp)?;
    } else {
        let comp = file.component_with(|c| c.uid() == &req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", req.uid));
        }
        checks(comp)?;

        file.create_overwrite(&req.uid, rid, locale.timezone(), |base, comp| {
            complete(Some(base), comp)
        })
        .context("Creating overwrite failed")?;
    }
    file.save()
        .context(format!("Save file {}:{:?}", req.uid, req.rid))?;

    Ok(Json(Response {}))
}
