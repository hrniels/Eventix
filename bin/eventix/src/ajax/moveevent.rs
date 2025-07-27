use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{NaiveDateTime, NaiveTime, TimeDelta, Timelike};
use eventix_ical::objects::{CalComponent, CalDate, CalDateTime, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::comps::date::Date;
use crate::pages::error::HTMLError;
use crate::util;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
    date: Date,
    hour: Option<u32>,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/moveevent", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = eventix_locale::default();
    let mut state = state.lock().await;

    let user_mail = util::user_for_uid(&state, &req.uid)?.map(|a| a.address());

    let rid = if let Some(rid) = &req.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {rid}"))?,
        )
    } else {
        None
    };

    let file = state
        .store_mut()
        .files_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    let get_timespan = |c: &CalComponent| -> anyhow::Result<(CalDate, CalDate)> {
        if !c.is_owned_by(user_mail.as_ref()) {
            return Err(anyhow!("No edit permission"));
        }

        let tz = locale.timezone();
        let duration = c.time_duration().unwrap();
        let old_start = c.start().unwrap().as_start_with_tz(tz);
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
            let new_time = if let Some(hour) = req.hour {
                NaiveTime::from_hms_opt(hour, old_start.minute(), old_start.second())
                    .ok_or_else(|| anyhow!("Invalid hour"))?
            } else {
                old_start.time()
            };

            let start = NaiveDateTime::new(new_date, new_time);
            let end = NaiveDateTime::new(new_date, new_time) + duration;
            Ok((
                CalDate::DateTime(CalDateTime::Timezone(start, tz.name().to_string())),
                CalDate::DateTime(CalDateTime::Timezone(end, tz.name().to_string())),
            ))
        }
    };

    let complete = |start: CalDate, end: CalDate, c: &mut CalComponent| {
        c.set_start(Some(start));
        c.as_event_mut().unwrap().set_end(Some(end));

        c.set_last_modified(CalDate::now());
        c.set_stamp(CalDate::now());
    };

    if let Some(comp) = file.component_with_mut(|c| c.uid() == &req.uid && c.rid() == rid.as_ref())
    {
        let (start, end) = get_timespan(comp)?;
        complete(start, end, comp);
    } else {
        let comp = file.component_with(|c| c.uid() == &req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", req.uid).into());
        }

        let (start, end) = get_timespan(comp)?;
        file.create_overwrite(&req.uid, rid.unwrap(), locale.timezone(), |_base, comp| {
            complete(start, end, comp)
        })
        .context("Creating overwrite failed")?;
    }
    file.save()
        .context(format!("Save file {}:{:?}", req.uid, req.rid))?;

    Ok(Json(Response {}))
}
