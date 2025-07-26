use anyhow::{Context, anyhow};
use axum::{
    Json, Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
};
use eventix_ical::objects::{
    CalComponent, CalDate, CalTodoStatus, EventLike, PRIORITY_MEDIUM, UpdatableEventLike,
};
use serde::{Deserialize, Serialize};

use crate::locale;
use crate::pages::error::HTMLError;
use crate::state::EventixState;

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/complete", get(handler))
        .with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();

    let rid = if let Some(rid) = req.rid.as_ref() {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {rid}"))?,
        )
    } else {
        None
    };

    let mut state = state.lock().await;

    let file = state
        .store_mut()
        .files_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    let complete = |c: &mut CalComponent| {
        let td = c.as_todo_mut().unwrap();
        td.set_status(Some(CalTodoStatus::Completed));
        td.set_percent(Some(100));
        td.set_completed(Some(CalDate::now()));
        // set the priority as is required by MS exchange as soon as TODOs are completed - unsure
        // why; we don't care about the priority at the moment and thus are fine with any value.
        td.set_priority(Some(PRIORITY_MEDIUM));
        td.set_last_modified(CalDate::now());
        td.set_stamp(CalDate::now());
    };

    if let Some(comp) = file.component_with_mut(|c| c.uid() == &req.uid && c.rid() == rid.as_ref())
    {
        complete(comp);
    } else {
        let comp = file.component_with(|c| c.uid() == &req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", req.uid).into());
        }

        file.create_overwrite(&req.uid, rid.unwrap(), locale.timezone(), |_base, comp| {
            complete(comp)
        })
        .context("Creating overwrite failed")?;
    }
    file.save()
        .context(format!("Save file {}:{:?}", req.uid, req.rid))?;

    Ok(Json(Response {}))
}
