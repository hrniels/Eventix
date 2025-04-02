use anyhow::{anyhow, Context};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use ical::objects::EventLike;
use serde::{Deserialize, Serialize};

use crate::error::HTMLError;
use crate::state::EventixState;
use crate::{locale, util};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/delete", post(handler))
        .with_state(state)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(form): Query<Request>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();

    let mut state = state.lock().await;

    {
        let occ = state
            .store()
            .occurrence_by_id(&form.uid, None, locale.timezone())
            .ok_or_else(|| anyhow!("Unable to find occurrence with uid {}", form.uid))?;
        if !util::user_is_event_owner(occ.directory(), &state, occ.organizer()) {
            return Err(anyhow!("No delete permission").into());
        }
    }

    let file = state.store_mut().files_by_id_mut(&form.uid).unwrap();

    let src = file.directory().clone();
    state
        .store_mut()
        .directory_mut(&src)
        .unwrap()
        .delete_by_uid(&form.uid)
        .with_context(|| format!("Unable to delete item with uid {}", form.uid))?;

    Ok(Json(Response {}))
}
