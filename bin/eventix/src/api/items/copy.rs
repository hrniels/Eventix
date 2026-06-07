// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{NaiveDateTime, NaiveTime, Timelike};
use eventix_ical::col::CalFile;
use eventix_ical::objects::{CalDate, CalDateTime, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::Deserialize;
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

type Response = ();

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
            .file_by_id_mut(&req.uid)
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

        let ctx = file.calendar().date_context();
        let old_start = comp
            .start()
            .ok_or_else(|| anyhow!("Event has no start date"))?
            .as_start_with_resolver(tz, file.calendar().timezone_resolver())
            .with_timezone(tz);

        let new_start = if comp.is_all_day() {
            CalDate::Date(new_date, comp.ctype().into())
        } else {
            let new_time = if let Some(hour) = req.hour {
                NaiveTime::from_hms_opt(hour, old_start.minute(), old_start.second())
                    .ok_or_else(|| anyhow!("Invalid hour"))?
            } else {
                old_start.time()
            };
            let start = NaiveDateTime::new(new_date, new_time);
            CalDate::DateTime(CalDateTime::Timezone(start, tz.name().to_string()))
        };

        let mut new_comp = comp.clone();
        new_comp
            .as_event_mut()
            .unwrap()
            .shift_to(&ctx, new_start, tz)
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
        .map_err(anyhow::Error::from)?;

    let mut path = dir_arc.path().clone();
    path.push(format!("{new_uid}.ics"));

    cal.add_component(new_comp);
    cal.populate_timezones();
    let new_file = CalFile::new(dir.clone(), path, cal);
    new_file.save().context(format!(
        "Unable to save copy of item with uid '{}' as '{}'",
        req.uid, new_uid
    ))?;

    dir_arc.add_file(new_file).map_err(anyhow::Error::from)?;

    Ok(Json(()))
}
