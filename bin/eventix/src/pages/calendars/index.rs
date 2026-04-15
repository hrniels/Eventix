// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use eventix_locale::Locale;
use eventix_state::{CollectionSettings, EventixState, SyncerType};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use tokio::{fs, io::AsyncReadExt};
use xdg::BaseDirectories;

use crate::{
    comps::calbox::{CalendarBox, CalendarBoxMode},
    html::filters,
};
use crate::{
    comps::{
        calbox::CalendarBoxTemplate,
        combobox::{ComboOption, ComboboxTemplate},
    },
    pages::error::HTMLError,
};

/// Fragment-only template for the calendars list, rendered by the AJAX content endpoint.
#[derive(Template)]
#[template(path = "pages/calendars.htm")]
struct CalendarsTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    collections: &'a BTreeMap<String, CollectionSettings>,
    calendars: BTreeMap<&'a String, Vec<CalendarBoxTemplate<'a>>>,
    collection_select: ComboboxTemplate<String>,
}

async fn metadata_or_default(dir: &Path, folder: &str, filename: &str, def: &str) -> String {
    let path = dir.join(folder).join(filename);
    let Ok(mut file) = fs::File::open(path).await else {
        return def.to_string();
    };
    let mut content = String::new();
    let Ok(_) = file.read_to_string(&mut content).await else {
        return def.to_string();
    };
    content
}

async fn add_unknown_calendars<'a>(
    xdg: &BaseDirectories,
    locale: &Arc<dyn Locale + Send + Sync>,
    col_id: &'a String,
    col: &'a CollectionSettings,
    known: &mut Vec<CalendarBoxTemplate<'a>>,
) -> anyhow::Result<()> {
    if let SyncerType::FileSystem { .. } = col.syncer() {
        return Ok(());
    }

    let col_path = col.path(xdg, col_id);
    let mut reader = fs::read_dir(&col_path)
        .await
        .context(format!("Reading directory '{:?}'", col_path))?;
    while let Some(f) = reader.next_entry().await? {
        let folder = f.file_name();
        let folder = folder.to_str().unwrap();
        if !known.iter().any(|cal| {
            if let CalendarBox::Known { settings, .. } = cal.cal() {
                settings.folder() == folder
            } else {
                false
            }
        }) {
            let id = uuid::Uuid::new_v4().simple().to_string();
            known.push(CalendarBoxTemplate::new(
                xdg,
                locale.clone(),
                col_id,
                col,
                CalendarBox::Unknown {
                    id,
                    folder: folder.to_string(),
                    name: metadata_or_default(&col_path, folder, "displayname", folder).await,
                    color: metadata_or_default(&col_path, folder, "color", "gray").await,
                },
                CalendarBoxMode::View,
            ));
        }
    }

    known.sort_by(|a, b| a.cal().name().cmp(b.cal().name()));
    Ok(())
}

/// Renders only the calendars list fragment. Used by the AJAX content endpoint.
pub async fn content(State(state): State<EventixState>) -> Result<impl IntoResponse, HTMLError> {
    let state = state.lock().await;
    let xdg = state.xdg();
    let locale = state.locale();

    let mut calendars = BTreeMap::new();
    for (col_id, col) in state.settings().collections() {
        let mut cals = col
            .all_calendars()
            .iter()
            .map(|(cal_id, cal)| {
                CalendarBoxTemplate::new(
                    xdg,
                    locale.clone(),
                    col_id,
                    col,
                    CalendarBox::Known {
                        id: cal_id,
                        settings: cal,
                    },
                    CalendarBoxMode::View,
                )
            })
            .collect::<Vec<_>>();
        if let Err(e) = add_unknown_calendars(xdg, &locale, col_id, col, &mut cals).await {
            tracing::error!(
                "Unable to determine calendars in collection {}: {}",
                col_id,
                e,
            );
        }
        calendars.insert(col_id, cals);
    }

    let collection_options = state
        .settings()
        .collections()
        .keys()
        .map(|col_id| ComboOption::new(col_id, col_id.clone()))
        .collect();

    let html = CalendarsTemplate {
        locale,
        collections: state.settings().collections(),
        calendars,
        collection_select: ComboboxTemplate::new_with_options(
            state.locale(),
            "new_calendar_collection",
            state.settings().collections().keys().next().cloned(),
            collection_options,
        ),
    }
    .render()
    .context("calendars content template")?;

    Ok(Html(html))
}
