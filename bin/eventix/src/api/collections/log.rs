use anyhow::Context;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use eventix_locale::DateFlags;
use eventix_state::EventixState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, BufReader};

use crate::api::JsonError;

#[derive(Debug, Deserialize)]
pub struct Params {
    col_id: String,
}

#[derive(Debug, Serialize)]
struct Response {
    html: String,
}

#[derive(Template)]
#[template(path = "ajax/log.htm")]
struct LogTemplate {
    title: String,
    content: String,
}

pub fn router(state: EventixState) -> Router {
    Router::new().route("/log", get(handler)).with_state(state)
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Params>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    let state = state.lock().await;
    let locale = state.settings().locale();

    let log_path = eventix_state::log_file(state.xdg(), &req.col_id);
    let date = log_path
        .metadata()
        .context("Get log metadata")?
        .modified()
        .context("Get log modification time")?;
    let date_utc: DateTime<Utc> = date.into();
    let date_local: DateTime<Tz> = date_utc.with_timezone(locale.timezone());
    let title = format!(
        "Log of collection '{}' from {}",
        req.col_id,
        locale.fmt_datetime(&date_local, DateFlags::None)
    );

    let file = OpenOptions::new()
        .read(true)
        .open(log_path)
        .await
        .context("Open log file")?;
    let mut reader = BufReader::new(file);
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .await
        .context("Reading log file")?;

    let html = LogTemplate { title, content }
        .render()
        .context("log template")?;

    Ok(Json(Response { html }))
}
