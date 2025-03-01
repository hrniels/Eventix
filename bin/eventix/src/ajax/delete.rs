use anyhow::anyhow;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use ical::col::CalStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::HTMLError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
}

#[derive(Debug, Serialize)]
struct Response {}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/delete", get(handler))
        .with_state(state)
}

async fn action_delete(store: Arc<Mutex<CalStore>>, form: &Request) -> anyhow::Result<()> {
    let mut store = store.lock().await;
    let file = store
        .files_by_id_mut(&form.uid)
        .ok_or_else(|| anyhow!("Unable to find file with uid {}", form.uid))?;

    let src = file.source().clone();
    store.source_mut(&src).unwrap().delete_by_uid(&form.uid)?;

    Ok(())
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(form): Query<Request>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    action_delete(state.store().clone(), &form).await?;

    Ok(Json(Response {}))
}
