// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::anyhow;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use eventix_state::{CalendarSettings, EventixState, create_calendar_by_folder};
use serde::Deserialize;
use std::path::Path;

use crate::api::JsonError;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/addcal", post(handler))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct Params {
    col_id: String,
    name: String,
}

fn sanitize_folder_name(name: &str) -> String {
    let mut folder = String::with_capacity(name.len());
    let mut last_was_sep = false;

    for ch in name.trim().chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            folder.push(lower);
            last_was_sep = false;
            continue;
        }

        if matches!(lower, '-' | '_' | '.') && !folder.is_empty() && !last_was_sep {
            folder.push(lower);
            last_was_sep = matches!(lower, '-' | '_');
            continue;
        }

        if !folder.is_empty() && !last_was_sep {
            folder.push('-');
            last_was_sep = true;
        }
    }

    let folder = folder
        .trim_matches(|ch| matches!(ch, '-' | '_' | '.'))
        .to_string();

    if folder.is_empty() {
        uuid::Uuid::new_v4().simple().to_string()
    } else {
        folder
    }
}

async fn unique_folder_name(
    col_path: &Path,
    calendars: &std::collections::BTreeMap<String, CalendarSettings>,
    name: &str,
) -> String {
    let base = sanitize_folder_name(name);
    let mut candidate = base.clone();
    let mut suffix = 2;

    loop {
        let known = calendars
            .values()
            .any(|settings| settings.folder() == &candidate);
        let exists = tokio::fs::try_exists(col_path.join(&candidate))
            .await
            .unwrap_or(false);
        if !known && !exists {
            return candidate;
        }

        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Params>,
) -> anyhow::Result<impl IntoResponse, JsonError> {
    let mut state = state.lock().await;
    let locale = state.locale();
    let xdg = state.xdg().clone();

    let name = req.name.trim();
    if name.is_empty() {
        return Err(anyhow!(locale.translate("error.calendar_name").to_string()).into());
    }

    let (folder, col_path) = {
        let col = state
            .settings_mut()
            .collections_mut()
            .get_mut(&req.col_id)
            .ok_or_else(|| anyhow!("No collection '{}'", &req.col_id))?;
        let col_path = col.path(&xdg, &req.col_id);
        let folder = unique_folder_name(&col_path, col.all_calendars(), name).await;
        (folder, col_path)
    };

    let col = state
        .settings_mut()
        .collections_mut()
        .get_mut(&req.col_id)
        .ok_or_else(|| anyhow!("No collection '{}'", &req.col_id))?;

    let id = uuid::Uuid::new_v4().simple().to_string();
    let mut cal = CalendarSettings::default();
    cal.set_enabled(true);
    cal.set_folder(folder.clone());
    cal.set_name(name.to_string());
    cal.set_bgcolor("#555555".to_string());
    cal.set_fgcolor("#ffffff".to_string());
    col.all_calendars_mut().insert(id, cal);

    // Keep freshly added settings visible until the next real sync discovers metadata.
    if let Err(e) = tokio::fs::create_dir_all(col_path.join(&folder)).await {
        tracing::debug!(
            "Unable to create calendar directory for '{}': {}",
            req.col_id,
            e
        );
    }

    create_calendar_by_folder(&mut state, &req.col_id, &folder).await?;

    if let Err(e) = state.settings().write_to_file() {
        tracing::warn!("Unable to save settings: {}", e);
    }

    eventix_state::State::refresh_store(&mut state).await?;

    Ok(Json(()))
}

#[cfg(test)]
mod tests {
    use super::sanitize_folder_name;

    #[test]
    fn sanitize_folder_name_normalizes_punctuation() {
        assert_eq!(
            sanitize_folder_name("Team Calendar / QA"),
            "team-calendar-qa"
        );
        assert_eq!(
            sanitize_folder_name("Roadmap_2026.Final"),
            "roadmap_2026.final"
        );
    }

    #[test]
    fn sanitize_folder_name_falls_back_for_empty_results() {
        let sanitized = sanitize_folder_name(" !!! ");
        assert_eq!(sanitized.len(), 32);
        assert!(sanitized.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
}
