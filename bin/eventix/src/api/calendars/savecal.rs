// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::anyhow;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_ical::objects::CalCompType;
use eventix_state::{CalendarAlarmType, CalendarSettings, EventixState};
use serde::Deserialize;

use crate::api::{JsonError, run_post};
use crate::comps::alarmconfig::AlarmConfig;
use crate::comps::calbox::AlarmType;
use crate::extract::MultiForm;

#[derive(Debug, Deserialize)]
pub struct Params {
    col_id: String,
    cal_id: String,
}

#[derive(Debug, Deserialize)]
pub struct PostData {
    name: String,
    folder: String,
    bgcolor: String,
    fgcolor: String,
    ev_types: Option<Vec<CalCompType>>,
    alarm_type: AlarmType,
    alarms: AlarmConfig,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/savecal", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Params>,
    MultiForm(form): MultiForm<PostData>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    run_post(state, move |state| Box::pin(run_savecal(state, req, form))).await
}

async fn run_savecal(
    state: &mut eventix_state::State,
    req: Params,
    form: PostData,
) -> anyhow::Result<Json<()>> {
    let locale = state.locale();

    let col = state
        .settings_mut()
        .collections_mut()
        .get_mut(&req.col_id)
        .ok_or_else(|| anyhow!("No collection '{}'", &req.col_id))?;

    let update_cal = |settings: &mut CalendarSettings| -> anyhow::Result<()> {
        settings.set_name(form.name.clone());
        settings.set_fgcolor(form.fgcolor.clone());
        settings.set_bgcolor(form.bgcolor.clone());
        settings.set_folder(form.folder.clone());
        settings.set_types(form.ev_types.clone().unwrap_or_default());
        let alarms = match form.alarm_type {
            AlarmType::Calendar => CalendarAlarmType::Calendar,
            AlarmType::Personal => CalendarAlarmType::Personal {
                default: if let Some(alarms) = form.alarms.to_alarms(locale.timezone().name())? {
                    alarms.into_iter().next()
                } else {
                    None
                },
            },
        };
        settings.set_alarms(alarms);
        Ok(())
    };

    if let Some(cal) = col.all_calendars_mut().get_mut(&req.cal_id) {
        update_cal(cal)?;
    } else {
        let mut cal = CalendarSettings::default();
        cal.set_enabled(true);
        update_cal(&mut cal)?;
        col.all_calendars_mut().insert(req.cal_id, cal);
    }

    if let Err(e) = state.settings().write_to_file() {
        tracing::warn!("Unable to save settings: {}", e);
    }

    eventix_state::State::refresh_store(state).await?;

    Ok(Json(()))
}
