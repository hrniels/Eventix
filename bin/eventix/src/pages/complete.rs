use anyhow::{anyhow, Context};
use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use ical::objects::{CalComponent, CalDate, CalTodoStatus, EventLike};
use serde::{Deserialize, Serialize};

use crate::{error::HTMLError, locale};

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn path() -> &'static str {
    "/complete"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new().route("/", get(handler)).with_state(state)
}

async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();

    let rid = if let Some(rid) = req.rid.as_ref() {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {}", rid))?,
        )
    } else {
        None
    };

    let mut store = state.store().lock().await;

    let item = store
        .item_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    let complete = |c: &mut CalComponent| {
        let td = c.as_todo_mut().unwrap();
        td.set_status(Some(CalTodoStatus::Completed));
        td.set_percent(Some(100));
        td.set_completed(Some(CalDate::now()));
    };

    if let Some(comp) = item.component_with_mut(|c| c.uid() == &req.uid && c.rid() == rid.as_ref())
    {
        complete(comp);
    } else {
        let comp = item.component_with(|c| c.uid() == &req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", req.uid).into());
        }

        item.overwrite_component(rid.unwrap(), locale.timezone(), complete);
    }
    item.save()
        .context(format!("Save item {}:{:?}", req.uid, req.rid))?;

    Ok(Json(Response {}))
}
