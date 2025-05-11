use anyhow::{Context, Result};
use askama::Template;
use axum::Router;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json, Redirect};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};

use ical::objects::CalDate;

use crate::comps::{alarmconfig::AlarmConfig, editalarm::EditAlarmTemplate};
use crate::extract::MultiForm;
use crate::locale;
use crate::pages::error::HTMLError;

use crate::state::EventixState;

#[derive(Debug, Deserialize)]
pub struct GetRequest {
    uid: String,
    rid: Option<String>,
    edit: bool,
}

#[derive(Debug, Serialize)]
struct GetResponse {
    html: String,
}

#[derive(Debug, Deserialize)]
pub struct PostRequest {
    uid: String,
    rid: Option<String>,
    target_url: String,
    #[serde(default)]
    personal: AlarmConfig,
    personal_overwrite: Option<String>,
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/editalarm", get(get_handler))
        .route("/editalarm", post(post_handler))
        .with_state(state)
}

pub async fn get_handler(
    State(state): State<EventixState>,
    Query(req): Query<GetRequest>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();

    let state = state.lock().await;

    let html = EditAlarmTemplate::new(locale, &state, req.uid, req.rid, req.edit)?
        .render()
        .context("details template")?;

    Ok(Json(GetResponse { html }))
}

pub async fn post_handler(
    State(state): State<EventixState>,
    MultiForm(req): MultiForm<PostRequest>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();

    let rid = if let Some(rid) = &req.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {}", rid))?,
        )
    } else {
        None
    };

    let mut state = state.lock().await;

    let occ = state
        .store()
        .occurrence_by_id(&req.uid, rid.as_ref(), locale.timezone())
        .context(format!(
            "No occurrence with uid={}, rid={:?}",
            req.uid, req.rid
        ))?;
    let calendar = occ.directory().clone();
    let personal_alarms = state.personal_alarms_mut();

    let pers_cal = personal_alarms.get_or_create(&calendar);
    let changed = if req.personal_overwrite.is_some() {
        pers_cal.set(
            &req.uid,
            rid.as_ref(),
            req.personal.to_alarms(&locale)?.unwrap_or_default(),
        )
    } else {
        pers_cal.unset(&req.uid, rid.as_ref())
    };

    if changed {
        pers_cal
            .save()
            .context(format!("Save personal alarms for calendar {}", calendar))?;
    }

    Ok(Redirect::to(&req.target_url))
}
