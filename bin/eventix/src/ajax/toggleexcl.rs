use anyhow::{anyhow, Context};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use ical::col::CalStore;
use ical::objects::{CalDate, EventLike, UpdatableEventLike};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::HTMLError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
    rid: String,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/toggleexcl", get(handler))
        .with_state(state)
}

async fn action_delete(store: Arc<Mutex<CalStore>>, form: &Request) -> anyhow::Result<()> {
    let mut store = store.lock().await;
    let item = store
        .item_by_id_mut(&form.uid)
        .ok_or_else(|| anyhow!("Unable to find item with uid {}", form.uid))?;

    let date = form
        .rid
        .parse::<CalDate>()
        .context(format!("Invalid rid date: {}", form.rid))?;

    let base = item
        .component_with_mut(|c| c.rid().is_none() && c.uid() == &form.uid)
        .ok_or_else(|| anyhow!("Unable to find base component with uid {}", form.uid))?;

    base.toggle_exclude(date);
    item.save()?;

    Ok(())
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(form): Query<Request>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    action_delete(state.store().clone(), &form).await?;

    Ok(Json(Response {}))
}
