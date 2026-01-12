use anyhow::Context;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use eventix_locale::{DateFlags, Locale};
use eventix_state::EventixState;
use formatx::formatx;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
    let locale = state.locale();

    let log_path = eventix_state::log_file(state.xdg(), &req.col_id);

    let (title, content) = if log_path.exists() {
        log_info(&locale, &req.col_id, &log_path).await?
    } else {
        (
            formatx!(locale.translate("Log of collection '{}'"), req.col_id).unwrap(),
            format!("- {} -", locale.translate("No entries")),
        )
    };

    let html = LogTemplate { title, content }
        .render()
        .context("log template")?;

    Ok(Json(Response { html }))
}

async fn log_info(
    locale: &Arc<dyn Locale + Send + Sync>,
    col_id: &String,
    log_path: &PathBuf,
) -> anyhow::Result<(String, String)> {
    let date = log_path
        .metadata()
        .context("Get log metadata")?
        .modified()
        .context("Get log modification time")?;
    let date_utc: DateTime<Utc> = date.into();
    let date_local: DateTime<Tz> = date_utc.with_timezone(locale.timezone());
    let title = formatx!(
        locale.translate("Log of collection '{}' from {}"),
        col_id,
        locale.fmt_datetime(&date_local, DateFlags::None)
    )
    .unwrap();

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

    Ok((title, content))
}
