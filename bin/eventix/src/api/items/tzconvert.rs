// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::anyhow;
use axum::extract::Query;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use chrono_tz::Tz;
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::api::JsonError;

#[derive(Debug, Deserialize)]
pub struct Request {
    from_date: Option<String>,
    from_time: Option<String>,
    to_date: Option<String>,
    to_time: Option<String>,
    from_tz: String,
    to_tz: String,
}

#[derive(Debug, Serialize)]
struct Response {
    from_date: Option<String>,
    from_time: Option<String>,
    to_date: Option<String>,
    to_time: Option<String>,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/tzconvert", get(handler))
        .with_state(state)
}

/// Parses a date string in `YYYY-MM-DD` format.
fn parse_date(s: &str) -> anyhow::Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| anyhow!("Invalid date '{}': {}", s, e))
}

/// Parses a time string in `HH:MM` format.
fn parse_time(s: &str) -> anyhow::Result<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M").map_err(|e| anyhow!("Invalid time '{}': {}", s, e))
}

/// Parses an IANA timezone name.
fn parse_tz(s: &str) -> anyhow::Result<Tz> {
    s.parse::<Tz>()
        .map_err(|_| anyhow!("Unknown timezone '{}'", s))
}

/// Converts a date+time pair from one timezone to another.
///
/// Returns the converted `(date_string, time_string)` pair formatted
/// as `YYYY-MM-DD` and `HH:MM` respectively.
fn convert(date_str: &str, time_str: &str, from: &Tz, to: &Tz) -> anyhow::Result<(String, String)> {
    let date = parse_date(date_str)?;
    let time = parse_time(time_str)?;
    let naive = NaiveDateTime::new(date, time);
    let in_from = from
        .from_local_datetime(&naive)
        .earliest()
        .ok_or_else(|| anyhow!("Ambiguous or invalid time in {}", from))?;
    let in_to = in_from.with_timezone(to);
    Ok((
        in_to.format("%Y-%m-%d").to_string(),
        in_to.format("%H:%M").to_string(),
    ))
}

async fn handler(Query(req): Query<Request>) -> anyhow::Result<impl IntoResponse, JsonError> {
    let from_tz = parse_tz(&req.from_tz)?;
    let to_tz = parse_tz(&req.to_tz)?;

    let (from_date, from_time) = match (req.from_date.as_deref(), req.from_time.as_deref()) {
        (Some(d), Some(t)) if !d.is_empty() && !t.is_empty() => {
            let (d, t) = convert(d, t, &from_tz, &to_tz)?;
            (Some(d), Some(t))
        }
        _ => (req.from_date, req.from_time),
    };

    let (to_date, to_time) = match (req.to_date.as_deref(), req.to_time.as_deref()) {
        (Some(d), Some(t)) if !d.is_empty() && !t.is_empty() => {
            let (d, t) = convert(d, t, &from_tz, &to_tz)?;
            (Some(d), Some(t))
        }
        _ => (req.to_date, req.to_time),
    };

    Ok(Json(Response {
        from_date,
        from_time,
        to_date,
        to_time,
    }))
}
