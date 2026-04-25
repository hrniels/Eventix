// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{NaiveDateTime, NaiveTime, TimeDelta, Timelike};
use eventix_ical::col::CalFile;
use eventix_ical::objects::{CalDate, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::{JsonError, run_post};
use crate::comps::date::Date;
use crate::util;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    uid: String,
    date: Date,
    hour: Option<u32>,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/copy", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    run_post(state, move |state| Box::pin(run_copy(state, req))).await
}

async fn run_copy(
    state: &mut eventix_state::State,
    req: Request,
) -> anyhow::Result<Json<Response>> {
    let locale = state.locale();

    let user_mail = util::user_for_uid(state, &req.uid)?.map(|a| a.address());

    let tz = locale.timezone();
    let new_date = req.date.date().ok_or_else(|| anyhow!("Invalid date"))?;

    let (dir, mut cal, mut new_comp) = {
        let file = state
            .store_mut()
            .files_by_id_mut(&req.uid)
            .context(format!("Unable to find component with uid '{}'", req.uid))?;

        let comp = file
            .component_with(|c| c.uid() == &req.uid)
            .ok_or_else(|| anyhow!("Component '{}' not found in file", req.uid))?;

        if !comp.is_owned_by(user_mail.as_ref()) {
            return Err(anyhow!("No edit permission"));
        }
        if comp.is_recurrent() {
            return Err(anyhow!("Copying recurrent events is not supported"));
        }

        let duration = comp
            .time_duration()
            .ok_or_else(|| anyhow!("Event has no duration"))?;
        let ctx = file.calendar().date_context();
        let old_start = comp
            .start()
            .ok_or_else(|| anyhow!("Event has no start date"))?
            .as_start_with_resolver(tz, ctx.resolver())
            .with_timezone(tz);

        let (new_start, new_end) = if comp.is_all_day() {
            // add one second here as the duration for all-day events is one second less to stay on
            // the same day.
            let end = new_date + (duration + TimeDelta::seconds(1));
            (
                CalDate::Date(new_date, comp.ctype().into()),
                CalDate::Date(end, comp.ctype().into()),
            )
        } else {
            let source_start = comp.start().unwrap();
            let source_end = comp.end_or_due().unwrap();
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
            (
                source_start
                    .from_resolved_in_tz(start_instant, tz, ctx.resolver())
                    .map_err(anyhow::Error::from)?,
                source_end
                    .from_resolved_in_tz(end_instant, tz, ctx.resolver())
                    .map_err(anyhow::Error::from)?,
            )
        };

        let mut new_comp = comp.clone();
        new_comp
            .set_start_checked(Some(new_start), &ctx, tz)
            .map_err(anyhow::Error::from)?;
        new_comp
            .set_end_checked(Some(new_end), &ctx, tz)
            .map_err(anyhow::Error::from)?;

        let mut cal = file.calendar().clone();
        cal.delete_components(|_| true);

        (file.directory().clone(), cal, new_comp)
    };

    let new_uid = Uuid::new_v4().to_string();
    new_comp.set_uid(new_uid.clone());
    new_comp.set_last_modified(CalDate::now());
    new_comp.set_stamp(CalDate::now());

    let dir_arc = state
        .store_mut()
        .directory_mut(&dir)
        .ok_or_else(|| anyhow!("Unable to find directory with id {}", dir))?;

    let mut path = dir_arc.path().clone();
    path.push(format!("{new_uid}.ics"));

    cal.add_component(new_comp);
    cal.populate_timezones();
    let new_file = CalFile::new(dir.clone(), path, cal);
    new_file
        .save()
        .context(format!("Save copy of {} as {}", req.uid, new_uid))?;

    dir_arc.add_file(new_file);

    Ok(Json(Response {}))
}
