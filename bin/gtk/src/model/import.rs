// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Import previews and persisted, UID-preserving calendar imports.

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

#[derive(Clone, Debug)]
pub(crate) struct PreparedImport {
    pub calendar: Calendar,
    pub calendars: Vec<ImportCalendar>,
    pub duplicates: Vec<(String, String, String)>,
}

impl PreparedImport {
    pub fn prepare(state: &State, input: &str) -> anyhow::Result<Self> {
        let calendar: Calendar = input.parse()?;
        Self::from_calendar(state, calendar)
    }

    fn from_calendar(state: &State, calendar: Calendar) -> anyhow::Result<Self> {
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

        Ok(Self {
            calendar,
            calendars,
            duplicates,
        })
    }

    /// Rechecks destination policy and imports the preview's content, replacing current items.
    pub fn apply(&self, state: &mut State, destination: &str) -> anyhow::Result<()> {
        let current = Self::from_calendar(state, self.calendar.clone())?;
        if !current.calendars.iter().any(|c| c.id == destination) {
            bail!(
                state
                    .locale()
                    .translate("error.import_no_calendar")
                    .to_string()
            );
        }
        let key = Arc::new(destination.to_string());
        let dir = state.store_mut().directory_mut(&key)?;
        let result = dir.import(self.calendar.clone());
        state.notify_external_change();
        result.map_err(anyhow::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use eventix_ical::{
        col::CalDir,
        objects::{CalCompType, EventLike},
    };
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};

    use super::PreparedImport;

    fn state(path: &Path) -> State {
        let xdg = Arc::new(eventix_state::with_test_xdg(path, path));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: path.display().to_string(),
        });
        for (id, types) in [
            ("mixed", vec![CalCompType::Event, CalCompType::Todo]),
            ("events", vec![CalCompType::Event]),
        ] {
            let directory = path.join(id);
            fs::create_dir(&directory).unwrap();
            state.store_mut().add(CalDir::new_empty(
                Arc::new(id.into()),
                directory,
                id.into(),
                false,
            ));
            let mut settings = CalendarSettings::default();
            settings.set_enabled(true);
            settings.set_types(types);
            settings.set_name(id.into());
            collection.all_calendars_mut().insert(id.into(), settings);
        }
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        state
    }

    fn event(uid: &str, summary: &str) -> String {
        format!(
            "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTART:20261109T090000Z\r\n\
            DTEND:20261109T100000Z\r\nSUMMARY:{summary}\r\nEND:VEVENT\r\n"
        )
    }

    fn calendar(components: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{components}END:VCALENDAR\r\n")
    }

    #[test]
    fn restricts_destinations_to_calendars_supporting_all_imported_types() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path());
        let input = calendar(&format!(
            "{}BEGIN:VTODO\r\nUID:task\r\nSUMMARY:Task\r\nEND:VTODO\r\n",
            event("event", "Event")
        ));
        let preview = PreparedImport::prepare(&state, &input).unwrap();
        assert_eq!(preview.calendars.len(), 1);
        assert_eq!(preview.calendars[0].id, "mixed");
        assert!(preview.apply(&mut state, "events").is_err());
    }

    #[test]
    fn duplicate_preview_only_offers_the_existing_calendar() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path());
        PreparedImport::prepare(&state, &calendar(&event("same", "Original")))
            .unwrap()
            .apply(&mut state, "mixed")
            .unwrap();
        let preview =
            PreparedImport::prepare(&state, &calendar(&event("same", "Replacement"))).unwrap();
        assert_eq!(preview.duplicates.len(), 1);
        assert_eq!(
            preview.duplicates[0],
            ("same".into(), "mixed".into(), "mixed".into())
        );
        assert_eq!(preview.calendars.len(), 1);
        assert_eq!(preview.calendars[0].id, "mixed");
    }

    #[test]
    fn replaces_items_that_changed_or_appeared_since_the_preview() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path());
        let preview =
            PreparedImport::prepare(&state, &calendar(&event("same", "Preview"))).unwrap();
        PreparedImport::prepare(&state, &calendar(&event("same", "Newer")))
            .unwrap()
            .apply(&mut state, "mixed")
            .unwrap();
        preview.apply(&mut state, "mixed").unwrap();
        assert_eq!(
            state.store().file_by_id("same").unwrap().components()[0]
                .summary()
                .map(String::as_str),
            Some("Preview")
        );
        let preview =
            PreparedImport::prepare(&state, &calendar(&event("same", "Replacement"))).unwrap();
        PreparedImport::prepare(&state, &calendar(&event("same", "Another change")))
            .unwrap()
            .apply(&mut state, "mixed")
            .unwrap();
        preview.apply(&mut state, "mixed").unwrap();
        assert_eq!(
            state.store().file_by_id("same").unwrap().components()[0]
                .summary()
                .map(String::as_str),
            Some("Replacement")
        );
    }

    #[test]
    fn rejects_duplicates_spanning_different_calendars() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path());
        for (uid, target) in [("a", "mixed"), ("b", "events")] {
            PreparedImport::prepare(&state, &calendar(&event(uid, uid)))
                .unwrap()
                .apply(&mut state, target)
                .unwrap();
        }
        let input = calendar(&format!("{}{}", event("a", "A"), event("b", "B")));
        assert!(PreparedImport::prepare(&state, &input).is_err());
    }

    #[test]
    fn rejects_empty_and_exception_only_previews() {
        let tmp = tempfile::tempdir().unwrap();
        let state = state(tmp.path());
        for input in [
            calendar(""),
            calendar(
                "BEGIN:VEVENT\r\nUID:orphan\r\nRECURRENCE-ID:20261110T090000Z\r\nDTSTART:20261110T100000Z\r\nEND:VEVENT\r\n",
            ),
        ] {
            assert!(PreparedImport::prepare(&state, &input).is_err());
        }
    }

    #[test]
    fn revalidates_disabled_and_write_protected_destinations() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path());
        let preview = PreparedImport::prepare(&state, &calendar(&event("new", "New"))).unwrap();
        let guard = state
            .store_mut()
            .protect_directories([Arc::new("mixed".into())])
            .unwrap();
        assert!(preview.apply(&mut state, "mixed").is_err());
        drop(guard);
        state
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .all_calendars_mut()
            .get_mut("mixed")
            .unwrap()
            .set_enabled(false);
        assert!(preview.apply(&mut state, "mixed").is_err());
        assert_eq!(state.store().files().count(), 0);
    }

    #[test]
    fn revalidates_duplicate_locations_without_comparing_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = state(tmp.path());
        let preview = PreparedImport::prepare(&state, &calendar(&event("new", "Preview"))).unwrap();
        PreparedImport::prepare(&state, &calendar(&event("new", "Elsewhere")))
            .unwrap()
            .apply(&mut state, "events")
            .unwrap();
        assert!(preview.apply(&mut state, "mixed").is_err());
        assert_eq!(state.store().files().count(), 1);
        preview.apply(&mut state, "events").unwrap();
    }
}
