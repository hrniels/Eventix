use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_ical::objects::{CalDate, EventLike, UpdatableEventLike};
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};

use crate::api::JsonError;
use crate::util;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
    rid: String,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/toggle", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(form): Query<Request>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    let mut state = state.lock().await;

    let user_mail = util::user_for_uid(&state, &form.uid)?.map(|a| a.address());

    let file = state.store_mut().files_by_id_mut(&form.uid).unwrap();

    let date = form
        .rid
        .parse::<CalDate>()
        .context(format!("Invalid rid date: {}", form.rid))?;

    let base = file
        .component_with_mut(|c| c.rid().is_none() && c.uid() == &form.uid)
        .ok_or_else(|| anyhow!("Unable to find base component with uid {}", form.uid))?;

    if !base.is_owned_by(user_mail.as_ref()) {
        return Err(anyhow!("No edit permission").into());
    }

    base.toggle_exclude(date);
    base.set_last_modified(CalDate::now());
    base.set_stamp(CalDate::now());
    file.save()
        .with_context(|| format!("Unable to save item with uid {}", form.uid))?;

    Ok(Json(Response {}))
}