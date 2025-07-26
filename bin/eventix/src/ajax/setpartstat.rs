use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_ical::objects::{
    CalAttendee, CalComponent, CalDate, CalPartStat, EventLike, UpdatableEventLike,
};
use eventix_state::EventixState;
use serde::{Deserialize, Deserializer, Serialize};

use crate::pages::error::HTMLError;
use crate::{locale, util};

fn deserialize_partstat<'de, D>(deserializer: D) -> Result<CalPartStat, D::Error>
where
    D: Deserializer<'de>,
{
    let buf = String::deserialize(deserializer)?;
    match buf.as_str() {
        "Accept" => Ok(CalPartStat::Accepted),
        "Decline" => Ok(CalPartStat::Declined),
        "Tentative" => Ok(CalPartStat::Tentative),
        _ => Err(serde::de::Error::custom("invalid part status")),
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
    #[serde(deserialize_with = "deserialize_partstat")]
    stat: CalPartStat,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/setpartstat", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let mut state = state.lock().await;

    let user = util::user_for_uid(&state, &req.uid)?
        .ok_or_else(|| anyhow!("Email account not specified"))?;

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

    let complete = |base: Option<&CalComponent>, c: &mut CalComponent| {
        let mut atts = c
            .attendees()
            .unwrap_or(base.and_then(|b| b.attendees()).unwrap_or(&[]))
            .to_vec();
        if let Some(att) = atts.iter_mut().find(|a| a.address() == user.address()) {
            att.set_part_stat(Some(req.stat));
        } else {
            let mut att = CalAttendee::new(user.address());
            att.set_common_name(user.name().clone());
            att.set_part_stat(Some(req.stat));
            atts.push(att);
        }
        c.set_attendees(Some(atts));
        c.set_last_modified(CalDate::now());
        c.set_stamp(CalDate::now());
    };

    if let Some(comp) = file.component_with_mut(|c| c.uid() == &req.uid && c.rid() == rid.as_ref())
    {
        complete(None, comp);
    } else {
        let comp = file.component_with(|c| c.uid() == &req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", req.uid).into());
        }

        file.create_overwrite(&req.uid, rid.unwrap(), locale.timezone(), |base, comp| {
            complete(Some(base), comp)
        })
        .context("Creating overwrite failed")?;
    }
    file.save()
        .context(format!("Save file {}:{:?}", req.uid, req.rid))?;

    Ok(Json(Response {}))
}
