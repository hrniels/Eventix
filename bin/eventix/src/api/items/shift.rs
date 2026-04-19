// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{NaiveDateTime, NaiveTime, TimeDelta, Timelike};
use eventix_ical::objects::{CalComponent, CalDate, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::api::JsonError;
use crate::comps::date::Date;
use crate::util;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<CalDate>,
    date: Date,
    hour: Option<u32>,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/shift", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    let mut state = state.lock().await;
    let locale = state.locale();

    let user_mail = util::user_for_uid(&state, &req.uid)?.map(|a| a.address());

    let file = state
        .store_mut()
        .files_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;
    let ctx = file.calendar().date_context();

    let get_timespan = |c: &CalComponent| -> anyhow::Result<(CalDate, CalDate)> {
        if !c.is_owned_by(user_mail.as_ref()) {
            return Err(anyhow!("No edit permission"));
        }

        let tz = locale.timezone();
        let duration = c.time_duration().unwrap();
        let old_start = ctx.date(c.start().unwrap()).start_in(tz);
        let new_date = req.date.date().ok_or_else(|| anyhow!("Invalid date"))?;

        if c.is_all_day() {
            // add one second here as the duration for all-day events is one second less to stay on
            // the same day.
            let end = new_date + (duration + TimeDelta::seconds(1));
            Ok((
                CalDate::Date(new_date, c.ctype().into()),
                CalDate::Date(end, c.ctype().into()),
            ))
        } else {
            let source_start = c.start().unwrap();
            let source_end = c.end_or_due().unwrap();
            let new_time = if let Some(hour) = req.hour {
                NaiveTime::from_hms_opt(hour, old_start.minute(), old_start.second())
                    .ok_or_else(|| anyhow!("Invalid hour"))?
            } else {
                old_start.time()
            };

            let start = NaiveDateTime::new(new_date, new_time);
            let end = start + duration;
            let start_instant =
                CalDate::resolve_local_datetime(start, tz).map_err(anyhow::Error::from)?;
            let end_instant =
                CalDate::resolve_local_datetime(end, tz).map_err(anyhow::Error::from)?;
            Ok((
                source_start
                    .from_resolved_in_tz(start_instant, tz, ctx.resolver())
                    .map_err(anyhow::Error::from)?,
                source_end
                    .from_resolved_in_tz(end_instant, tz, ctx.resolver())
                    .map_err(anyhow::Error::from)?,
            ))
        }
    };

    let complete = |start: CalDate, end: CalDate, c: &mut CalComponent| -> anyhow::Result<()> {
        let local_tz = locale.timezone();
        c.set_start_checked(Some(start), &ctx, local_tz)?;
        c.set_end_checked(Some(end), &ctx, local_tz)?;

        c.set_last_modified(CalDate::now());
        c.set_stamp(CalDate::now());
        Ok(())
    };

    if let Some(comp) =
        file.component_with_mut(|c| c.uid() == &req.uid && c.rid() == req.rid.as_ref())
    {
        let (start, end) = get_timespan(comp)?;
        complete(start, end, comp)?;
    } else {
        let comp = file.component_with(|c| c.uid() == &req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", req.uid).into());
        }

        let (start, end) = get_timespan(comp)?;
        file.create_overwrite(
            &req.uid,
            req.rid.clone().unwrap(),
            locale.timezone(),
            |_base, comp| complete(start, end, comp),
        )
        .context("Creating overwrite failed")?;
    }
    file.save()
        .context(format!("Save file {}:{:?}", req.uid, req.rid))?;

    Ok(Json(Response {}))
}
