// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use anyhow::bail;
use eventix_ical::objects::{Calendar, EventLike};
use eventix_state::State;

#[derive(Clone, Debug)]
pub(crate) struct ImportCalendar {
    pub id: String,
    pub name: String,
    pub color: String,
}

/// Parsed import content and the destination choices and duplicates shown in its preview.
#[derive(Clone, Debug)]
pub(crate) struct PreparedImport {
    pub calendar: Calendar,
    pub calendars: Vec<ImportCalendar>,
    pub duplicates: Vec<(String, String, String)>,
}

impl PreparedImport {
    /// Builds an import preview using the current destination policy and duplicate locations.
    pub(crate) fn from_calendar(state: &State, calendar: Calendar) -> anyhow::Result<Self> {
        let locale = state.locale();
        let groups = calendar.clone().split_by_uid();
        if groups.is_empty()
            || groups.iter().any(|group| {
                group
                    .components()
                    .iter()
                    .filter(|c| c.rid().is_none())
                    .count()
                    != 1
            })
        {
            bail!(
                locale
                    .translate("No importable events or tasks found.")
                    .to_string()
            );
        }

        let mut duplicates = Vec::new();
        for group in &groups {
            let uid = group.components()[0].uid();
            for dir in state.store().directories() {
                if dir.file_by_id(uid).is_some() {
                    duplicates.push((uid.clone(), (**dir.id()).clone(), dir.name().clone()));
                }
            }
        }
        if duplicates.iter().any(|(_, id, _)| id != &duplicates[0].1) {
            bail!(
                locale
                    .translate("error.import_multiple_calendars")
                    .to_string()
            );
        }

        let calendars = state
            .settings()
            .calendars()
            .filter_map(|(id, settings)| {
                let key = Arc::new(id.clone());
                let (collection, _) = state.settings().calendar(id)?;
                if collection.is_read_only()
                    || state.store().directory(&key).is_none()
                    || state.store().directory_write_protected(&key)
                    || duplicates
                        .first()
                        .is_some_and(|(_, existing, _)| existing != id)
                    || calendar
                        .components()
                        .iter()
                        .any(|c| !settings.types().contains(&c.ctype()))
                {
                    return None;
                }
                Some(ImportCalendar {
                    id: id.clone(),
                    name: settings.name().clone(),
                    color: settings.bgcolor().clone(),
                })
            })
            .collect();

        Ok(PreparedImport {
            calendar,
            calendars,
            duplicates,
        })
    }
}
