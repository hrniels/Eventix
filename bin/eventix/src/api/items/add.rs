// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Form, Json, Router};
use eventix_ical::objects::{CalCompType, CalDateType, UpdatableEventLike};
use eventix_state::EventixState;
use serde::Deserialize;

use crate::api::JsonError;
use crate::comps::date::Date;
use crate::objects::create_component;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    #[serde(rename = "quicktodo_calendar")]
    calendar: String,
    summary: String,
    due_date: Date,
}

pub fn router(state: EventixState) -> Router {
    Router::new().route("/add", post(handler)).with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Form(req): Form<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    let mut state = state.lock().await;
    let locale = state.locale();

    create_component(
        &mut state,
        &locale,
        &req.calendar,
        CalCompType::Todo,
        locale.timezone().name(),
        |_cal, _alarm_type, comp, _persalarms, _organizer, _locale| {
            comp.set_summary(Some(req.summary));
            if let Some(due_date) = req.due_date.to_caldate(CalDateType::Inclusive, true) {
                comp.set_due_checked(Some(due_date), locale.timezone())?;
            }
            Ok(())
        },
    )?;

    Ok(Json(()))
}
