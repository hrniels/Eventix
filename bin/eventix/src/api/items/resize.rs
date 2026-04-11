// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{Days, NaiveDateTime, NaiveTime};
use eventix_ical::col::Occurrence;
use eventix_ical::objects::{CalComponent, CalDate, CalDateTime, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::api::JsonError;
use crate::util;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<CalDate>,
    start_hour: Option<u32>,
    start_minute: Option<u32>,
    end_hour: Option<u32>,
    end_minute: Option<u32>,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/resize", post(handler))
        .with_state(state)
}

fn ensure_half_hour(min: u32, name: &str) -> anyhow::Result<()> {
    if min == 0 || min == 30 {
        Ok(())
    } else {
        Err(anyhow!("{} must be 0 or 30", name))
    }
}

/// Converts a (hour, minute) pair on a 30-minute grid to a `NaiveTime`.
///
/// The special value hour=24, minute=0 represents end-of-day midnight and is translated to
/// `NaiveTime::from_hms_opt(0, 0, 0)` — the caller is responsible for advancing the date by
/// one day in that case.  All other inputs are passed to `NaiveTime::from_hms_opt` directly.
fn half_hour_to_time(hour: u32, minute: u32) -> anyhow::Result<(NaiveTime, bool)> {
    if hour == 24 && minute == 0 {
        let t = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        Ok((t, true))
    } else {
        let t = NaiveTime::from_hms_opt(hour, minute, 0)
            .ok_or_else(|| anyhow!("Invalid time {:02}:{:02}", hour, minute))?;
        Ok((t, false))
    }
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    let mut state = state.lock().await;
    let locale = state.locale();

    // Validate that exactly one side (start or end) is being resized.
    let resize_start = req.start_hour.is_some() || req.start_minute.is_some();
    let resize_end = req.end_hour.is_some() || req.end_minute.is_some();
    if resize_start == resize_end {
        return Err(anyhow!("Exactly one of start or end must be provided").into());
    }

    // Validate that the provided hour/minute pair is complete and the minute is 0 or 30.
    if resize_start {
        let _ = req.start_hour.unwrap();
        ensure_half_hour(req.start_minute.unwrap(), "start_minute")?;
    } else {
        let hour = req.end_hour.unwrap();
        let min = req.end_minute.unwrap();
        // hour=24, minute=0 is the special sentinel for end-of-day midnight.
        if !(hour == 24 && min == 0) {
            ensure_half_hour(min, "end_minute")?;
        }
    }

    let user_mail = util::user_for_uid(&state, &req.uid)?.map(|a| a.address());

    let file = state
        .store_mut()
        .files_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    let get_timespan = |c: &Occurrence<'_>| -> anyhow::Result<(CalDate, CalDate)> {
        if !c.is_owned_by(user_mail.as_ref()) {
            return Err(anyhow!("No edit permission"));
        }
        if c.is_all_day() {
            return Err(anyhow!("Cannot resize all-day events"));
        }

        let tz = locale.timezone();
        let old_start = c.occurrence_start().unwrap();
        let old_end = c
            .occurrence_end()
            .ok_or_else(|| anyhow!("Event has no end time"))?;

        if resize_start {
            let (new_time, _) =
                half_hour_to_time(req.start_hour.unwrap(), req.start_minute.unwrap())?;
            let new_start = NaiveDateTime::new(old_start.date_naive(), new_time);
            if new_start >= old_end.naive_local() {
                return Err(anyhow!("New start must be before existing end"));
            }
            Ok((
                CalDate::DateTime(CalDateTime::Timezone(new_start, tz.name().to_string())),
                CalDate::DateTime(CalDateTime::Timezone(
                    old_end.naive_local(),
                    tz.name().to_string(),
                )),
            ))
        } else {
            let (new_time, next_day) =
                half_hour_to_time(req.end_hour.unwrap(), req.end_minute.unwrap())?;
            // Determine the logical end date: the day the event visually "ends on".  An end of
            // 00:00:00 on day X+1 is treated as end-of-day on X (matching occurrence_ends_on),
            // so we subtract one day in that case before applying the new time.
            let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
            let logical_end_date =
                if old_end.time() == midnight && old_end.date_naive() > old_start.date_naive() {
                    old_end
                        .date_naive()
                        .checked_sub_days(Days::new(1))
                        .ok_or_else(|| anyhow!("End date underflow"))?
                } else {
                    old_end.date_naive()
                };
            let end_date = if next_day {
                logical_end_date
                    .checked_add_days(Days::new(1))
                    .ok_or_else(|| anyhow!("End date overflow"))?
            } else {
                logical_end_date
            };
            let new_end = NaiveDateTime::new(end_date, new_time);
            if new_end <= old_start.naive_local() {
                return Err(anyhow!("New end must be after existing start"));
            }
            Ok((
                CalDate::DateTime(CalDateTime::Timezone(
                    old_start.naive_local(),
                    tz.name().to_string(),
                )),
                CalDate::DateTime(CalDateTime::Timezone(new_end, tz.name().to_string())),
            ))
        }
    };

    let complete = |start: CalDate, end: CalDate, c: &mut CalComponent| -> anyhow::Result<()> {
        let local_tz = locale.timezone();
        c.set_start_checked(Some(start), local_tz)?;
        c.set_end_checked(Some(end), local_tz)?;
        c.set_last_modified(CalDate::now());
        c.set_stamp(CalDate::now());
        Ok(())
    };

    // determine new start/end based on the to-be-resized occurrence
    let occ = file
        .occurrence_by_id(&req.uid, req.rid.as_ref(), locale.timezone())
        .ok_or_else(|| anyhow!("Occurrence for {} at {:?} not found", req.uid, req.rid))?;
    let (start, end) = get_timespan(&occ)?;

    if let Some(comp) =
        file.component_with_mut(|c| c.uid() == &req.uid && c.rid() == req.rid.as_ref())
    {
        complete(start, end, comp)?;
    } else {
        let comp = file.component_with(|c| c.uid() == &req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", req.uid).into());
        }

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
