use anyhow::anyhow;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_ical::objects::CalCompType;
use eventix_state::{CalendarAlarmType, CalendarSettings, EventixState};
use serde::Deserialize;

use crate::api::JsonError;
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
    let mut state = state.lock().await;

    let locale = state.locale();

    let col = state
        .settings_mut()
        .collections_mut()
        .get_mut(&req.col_id)
        .ok_or_else(|| anyhow!("No collection '{}'", &req.col_id))?;

    let update_cal = |settings: &mut CalendarSettings| -> anyhow::Result<()> {
        settings.set_name(form.name);
        settings.set_fgcolor(form.fgcolor);
        settings.set_bgcolor(form.bgcolor);
        settings.set_folder(form.folder);
        settings.set_types(form.ev_types.unwrap_or_default());
        let alarms = match form.alarm_type {
            AlarmType::Calendar => CalendarAlarmType::Calendar,
            AlarmType::Personal => CalendarAlarmType::Personal {
                default: if let Some(alarms) = form.alarms.to_alarms(&locale)? {
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
        update_cal(&mut cal)?;
        col.all_calendars_mut().insert(req.cal_id, cal);
    }

    if let Err(e) = state.settings().write_to_file() {
        tracing::warn!("Unable to save settings: {}", e);
    }

    eventix_state::State::refresh_store(&mut state).await?;

    Ok(Json(()))
}
