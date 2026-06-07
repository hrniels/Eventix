// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{Days, NaiveDateTime, NaiveTime};
use eventix_ical::col::Occurrence;
use eventix_ical::objects::{CalDate, CalDateTime, EventLike, RangeEdge, UpdatableEventLike};
use eventix_locale::Locale;
use eventix_state::EventixState;
use serde::Deserialize;

use crate::api::{JsonError, run_post};
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

type Response = ();

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

fn get_resize_op(
    c: &Occurrence<'_>,
    locale: &Arc<dyn Locale + Send + Sync>,
    req: &Request,
    user_mail: Option<&String>,
    resize_start: bool,
) -> anyhow::Result<(RangeEdge, CalDate)> {
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
    let start_dt = old_start.naive_local();
    let end_dt = old_end.naive_local();

    if resize_start {
        let (new_time, _) = half_hour_to_time(req.start_hour.unwrap(), req.start_minute.unwrap())?;
        let new_start = NaiveDateTime::new(start_dt.date(), new_time);
        if new_start >= end_dt {
            return Err(anyhow!("New start must be before existing end"));
        }
        Ok((
            RangeEdge::Start,
            CalDate::DateTime(CalDateTime::Timezone(new_start, tz.name().to_string())),
        ))
    } else {
        let (new_time, next_day) =
            half_hour_to_time(req.end_hour.unwrap(), req.end_minute.unwrap())?;
        // Determine the logical end date: the day the event visually "ends on".  An end of
        // 00:00:00 on day X+1 is treated as end-of-day on X (matching occurrence_ends_on),
        // so we subtract one day in that case before applying the new time.
        let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        let logical_end_date = if end_dt.time() == midnight && end_dt.date() > start_dt.date() {
            end_dt
                .date()
                .checked_sub_days(Days::new(1))
                .ok_or_else(|| anyhow!("End date underflow"))?
        } else {
            end_dt.date()
        };
        let end_date = if next_day {
            logical_end_date
                .checked_add_days(Days::new(1))
                .ok_or_else(|| anyhow!("End date overflow"))?
        } else {
            logical_end_date
        };
        let new_end = NaiveDateTime::new(end_date, new_time);
        if new_end <= start_dt {
            return Err(anyhow!("New end must be after existing start"));
        }
        Ok((
            RangeEdge::End,
            CalDate::DateTime(CalDateTime::Timezone(new_end, tz.name().to_string())),
        ))
    }
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    run_post(state, move |state| Box::pin(run_resize(state, req))).await
}

async fn run_resize(
    state: &mut eventix_state::State,
    req: Request,
) -> anyhow::Result<Json<Response>> {
    let locale = state.locale();

    // Validate that exactly one side (start or end) is being resized.
    let resize_start = req.start_hour.is_some() || req.start_minute.is_some();
    let resize_end = req.end_hour.is_some() || req.end_minute.is_some();
    if resize_start == resize_end {
        return Err(anyhow!("Exactly one of start or end must be provided"));
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

    let user_mail = util::user_for_uid(state, &req.uid)?.map(|a| a.address());

    let file = state
        .store_mut()
        .file_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;
    let ctx = file.calendar().date_context();

    // determine new start/end based on the to-be-resized occurrence
    let occ = file
        .occurrence_by_id(&req.uid, req.rid.as_ref(), locale.timezone())
        .ok_or_else(|| {
            anyhow!(
                "Unable to find occurrence with uid '{}' and rid '{:?}'",
                req.uid,
                req.rid
            )
        })?;
    let (edge, new_value) = get_resize_op(&occ, &locale, &req, user_mail.as_ref(), resize_start)?;

    file.change_single(
        &req.uid,
        req.rid.as_ref(),
        locale.timezone(),
        user_mail.as_ref(),
        true,
        |_base, comp| {
            comp.as_event_mut()
                .unwrap()
                .resize(&ctx, edge, new_value, locale.timezone())
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
