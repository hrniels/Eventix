// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Context;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{NaiveDateTime, NaiveTime, Timelike};
use eventix_ical::objects::{CalComponent, CalDate, CalDateTime, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::Deserialize;

use crate::api::{JsonError, run_post};
use crate::comps::date::Date;
use crate::util;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<CalDate>,
    date: Date,
    hour: Option<u32>,
}

type Response = ();

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/shift", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    run_post(state, move |state| Box::pin(run_shift(state, req))).await
}

async fn run_shift(
    state: &mut eventix_state::State,
    req: Request,
) -> anyhow::Result<Json<Response>> {
    let locale = state.locale();

    let user_mail = util::user_for_uid(state, &req.uid)?.map(|a| a.address());

    let file = state
        .store_mut()
        .file_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;
    let ctx = file.calendar().date_context();
    let resolver = ctx.resolver();

    let get_new_start = |c: &CalComponent| -> Result<CalDate, String> {
        let tz = locale.timezone();
        let old_start = c
            .start()
            .unwrap()
            .as_start_with_resolver(tz, resolver)
            .with_timezone(tz);
        let new_date = req.date.date().ok_or_else(|| "Invalid date".to_string())?;

        if c.is_all_day() {
            Ok(CalDate::Date(new_date, c.ctype().into()))
        } else {
            let new_time = if let Some(hour) = req.hour {
                NaiveTime::from_hms_opt(hour, old_start.minute(), old_start.second())
                    .ok_or_else(|| "Invalid hour".to_string())?
            } else {
                old_start.time()
            };

            let start = NaiveDateTime::new(new_date, new_time);
            Ok(CalDate::DateTime(CalDateTime::Timezone(
                start,
                tz.name().to_string(),
            )))
        }
    };

    file.change_single(
        &req.uid,
        req.rid.as_ref(),
        locale.timezone(),
        user_mail.as_ref(),
        |_base, comp| {
            let start = get_new_start(comp)?;
            comp.as_event_mut()
                .unwrap()
                .shift_to(&ctx, start, locale.timezone())
                .map_err(|e| e.to_string())?;
            comp.touch();
            Ok(())
        },
    )?;

    file.save().context(format!(
        "Unable to save item with uid '{}' and rid '{:?}'",
        req.uid, req.rid
    ))?;

    Ok(Json(()))
}
