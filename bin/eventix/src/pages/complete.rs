use anyhow::Context;
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
    rid: String,
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

    let rid = req
        .rid
        .parse::<CalDate>()
        .context(format!("Invalid rid date: {}", req.rid))?;

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

    if let Some(comp) = item.component_with_mut(|c| c.uid() == &req.uid && c.rid() == Some(&rid)) {
        complete(comp);
    } else {
        item.overwrite_component(rid, locale.timezone(), complete);
    }

    Ok(Json(Response {}))
}
