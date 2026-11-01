// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Read-only collection and calendar management data, independent of calendar visibility.

use std::{fs, path::Path, sync::Arc};

use eventix_ical::objects::{CalAlarm, CalCompType};
use eventix_locale::Locale;
use eventix_state::{CalendarAlarmType, CollectionSettings, State, SyncerType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    FileSystem,
    CalDav,
    O365,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calendar {
    /// Configured ID, or the folder name for an unconfigured calendar.
    pub id: String,
    pub configured: bool,
    pub name: String,
    pub background: String,
    pub foreground: String,
    pub events: bool,
    pub tasks: bool,
    pub personal_alarms: bool,
    pub default_alarm: Option<CalAlarm>,
    pub enabled: bool,
    pub folder: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collection {
    pub id: String,
    pub syncer: SyncerType,
    pub log_path: String,
    pub backend: Backend,
    pub account: Option<String>,
    pub reload: bool,
    pub discover: bool,
    pub create_calendar: bool,
    pub calendars: Vec<Calendar>,
}

pub struct Snapshot {
    pub collections: Vec<Collection>,
    pub locale: Arc<dyn Locale + Send + Sync>,
    /// Directory scan failures; configured calendars remain available.
    pub warnings: Vec<String>,
}

impl Snapshot {
    /// Loads all collections, including disabled and locally discovered calendars.
    ///
    /// Reads local metadata synchronously and must be called off the UI thread. Scan failures
    /// are returned as warnings rather than hiding a collection's configured calendars.
    pub fn load(state: &State) -> Self {
        let mut warnings = Vec::new();
        let collections = state
            .settings()
            .collections()
            .iter()
            .map(|(id, settings)| {
                let path = settings.path(state.xdg(), id);
                let (mut collection, warning) = load_collection(id, settings, &path);
                collection.log_path = settings
                    .log_file(state.xdg(), id)
                    .to_string_lossy()
                    .into_owned();
                if let Some(warning) = warning {
                    warnings.push(warning);
                }
                collection
            })
            .collect();
        Self {
            collections,
            locale: state.locale(),
            warnings,
        }
    }
}

fn load_collection(
    id: &str,
    settings: &CollectionSettings,
    path: &Path,
) -> (Collection, Option<String>) {
    let backend = match settings.syncer() {
        SyncerType::FileSystem { .. } => Backend::FileSystem,
        SyncerType::VDirSyncer { .. } => Backend::CalDav,
        SyncerType::O365 { .. } => Backend::O365,
    };
    let mut calendars: Vec<_> = settings
        .all_calendars()
        .iter()
        .map(|(id, cal)| Calendar {
            id: id.clone(),
            configured: true,
            name: cal.name().clone(),
            background: cal.bgcolor().clone(),
            foreground: cal.fgcolor().clone(),
            events: cal.types().contains(&CalCompType::Event),
            tasks: cal.types().contains(&CalCompType::Todo),
            personal_alarms: matches!(cal.alarms(), CalendarAlarmType::Personal { .. }),
            default_alarm: match cal.alarms() {
                CalendarAlarmType::Personal { default } => default.clone(),
                CalendarAlarmType::Calendar => None,
            },
            enabled: cal.enabled(),
            folder: cal.folder().clone(),
            path: path.join(cal.folder()).to_string_lossy().into_owned(),
        })
        .collect();
    let warning = if backend != Backend::FileSystem {
        discover(path, &mut calendars)
            .err()
            .map(|error| format!("{id}: {}: {error}", path.display()))
    } else {
        None
    };
    calendars.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.folder.cmp(&b.folder))
            .then_with(|| a.id.cmp(&b.id))
    });
    (
        Collection {
            id: id.into(),
            syncer: settings.syncer().clone(),
            log_path: String::new(),
            backend,
            account: settings
                .email()
                .map(|email| format!("{} <{}>", email.name(), email.org_address())),
            reload: settings.syncer().supports_reload(),
            discover: settings.syncer().supports_discover(),
            create_calendar: !settings.is_read_only() && settings.syncer().supports_mkrm(),
            calendars,
        },
        warning,
    )
}

fn discover(path: &Path, calendars: &mut Vec<Calendar>) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().into_owned();
        if calendars.iter().any(|cal| cal.folder == folder) {
            continue;
        }
        let metadata = |file: &str, fallback: &str| {
            fs::read_to_string(entry.path().join(file))
                .map(|text| text.trim().to_owned())
                .ok()
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| fallback.to_owned())
        };
        calendars.push(Calendar {
            id: folder.clone(),
            configured: false,
            name: metadata("displayname", &folder),
            background: metadata("color", "gray"),
            foreground: "#000000".into(),
            events: false,
            tasks: false,
            personal_alarms: false,
            default_alarm: None,
            enabled: false,
            path: entry.path().to_string_lossy().into_owned(),
            folder,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Backend, Calendar, Snapshot, discover, load_collection};
    use eventix_ical::objects::{CalAction, CalAlarm, CalCompType, CalRelated, CalTrigger};
    use eventix_state::{
        CalendarAlarmType, CalendarSettings, CollectionSettings, EmailAccount, SyncerType,
    };
    use std::{fs, path::Path};

    #[test]
    fn calendars_include_disabled_entries_and_sort_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = CollectionSettings::new(SyncerType::FileSystem {
            path: dir.path().to_string_lossy().into_owned(),
        });
        for (id, name, enabled) in [("a", "Zebra", true), ("b", "Alpha", false)] {
            let mut calendar = CalendarSettings::default();
            calendar.set_name(name.into());
            calendar.set_folder(id.into());
            calendar.set_enabled(enabled);
            calendar.set_fgcolor("#ffffff".into());
            calendar.set_types(vec![CalCompType::Event]);
            calendar.set_alarms(CalendarAlarmType::Personal {
                default: Some(CalAlarm::new(
                    CalAction::Display,
                    CalTrigger::Relative {
                        duration: chrono::Duration::minutes(-15).into(),
                        related: CalRelated::Start,
                    },
                )),
            });
            settings.all_calendars_mut().insert(id.into(), calendar);
        }
        fs::create_dir(dir.path().join("unconfigured")).unwrap();
        let (collection, warning) = load_collection("local", &settings, dir.path());
        assert!(warning.is_none());
        assert_eq!(collection.calendars.len(), 2);
        assert_eq!(collection.calendars[0].name, "Alpha");
        assert!(!collection.calendars[0].enabled);
        assert!(collection.calendars[1].enabled);
        assert!(collection.calendars.iter().all(|cal| cal.configured));
        let calendar = &collection.calendars[0];
        assert_eq!(calendar.foreground, "#ffffff");
        assert!(calendar.events);
        assert!(!calendar.tasks);
        assert!(calendar.personal_alarms);
        assert_eq!(
            calendar.default_alarm.as_ref(),
            match settings.all_calendars()["b"].alarms() {
                CalendarAlarmType::Personal { default } => default.as_ref(),
                CalendarAlarmType::Calendar => None,
            }
        );
    }

    #[test]
    fn discovery_reads_metadata_skips_known_folders_and_retains_stable_ids() {
        let dir = tempfile::tempdir().unwrap();
        for folder in ["known", "new", "fallback"] {
            fs::create_dir(dir.path().join(folder)).unwrap();
        }
        fs::write(dir.path().join("file"), "not a calendar").unwrap();
        fs::write(dir.path().join("new/displayname"), "Shared\n").unwrap();
        fs::write(dir.path().join("new/color"), "#123456\n").unwrap();
        let known = Calendar {
            id: "configured-id".into(),
            configured: true,
            name: "Shared".into(),
            background: "red".into(),
            foreground: "#000000".into(),
            events: false,
            tasks: false,
            personal_alarms: false,
            default_alarm: None,
            enabled: false,
            folder: "known".into(),
            path: String::new(),
        };
        let mut calendars = vec![known];
        discover(dir.path(), &mut calendars).unwrap();
        assert_eq!(calendars.len(), 3);
        let new = calendars.iter().find(|cal| cal.folder == "new").unwrap();
        assert_eq!(new.name, "Shared");
        assert_eq!(new.background, "#123456");
        assert!(!new.configured);
        assert_eq!(new.foreground, "#000000");
        assert!(!new.events && !new.tasks && !new.personal_alarms);
        assert!(new.default_alarm.is_none());
        let fallback = calendars
            .iter()
            .find(|cal| cal.folder == "fallback")
            .unwrap();
        assert_eq!(fallback.name, "fallback");
        assert_eq!(fallback.background, "gray");
        let previous = calendars.clone();
        discover(dir.path(), &mut calendars).unwrap();
        assert_eq!(calendars, previous);
        assert!(discover(&dir.path().join("missing"), &mut calendars).is_err());
        assert_eq!(calendars, previous);
    }

    #[test]
    fn empty_local_collection_remains_present() {
        let settings = CollectionSettings::new(SyncerType::FileSystem {
            path: "/unused".into(),
        });
        let (collection, warning) = load_collection("empty", &settings, Path::new("/unused"));
        assert_eq!(collection.id, "empty");
        assert!(collection.calendars.is_empty());
        assert!(warning.is_none());
    }

    #[test]
    fn remote_scan_failure_preserves_configured_calendars_and_collection_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = CollectionSettings::new(SyncerType::VDirSyncer {
            email: EmailAccount::new("User".into(), "User@Example.com".into()),
            url: "https://example.com".into(),
            read_only: false,
            username: None,
            password: None,
            time_span: Default::default(),
        });
        let mut calendar = CalendarSettings::default();
        calendar.set_folder("known".into());
        calendar.set_name("Known".into());
        settings
            .all_calendars_mut()
            .insert("calendar".into(), calendar);
        let (collection, warning) =
            load_collection("remote", &settings, &dir.path().join("missing"));
        assert!(warning.unwrap().contains("remote"));
        assert_eq!(collection.calendars.len(), 1);
        assert_eq!(collection.calendars[0].id, "calendar");
        assert_eq!(collection.backend, Backend::CalDav);
        assert_eq!(
            collection.account.as_deref(),
            Some("User <User@Example.com>")
        );
        assert!(collection.reload && collection.discover && collection.create_calendar);
        fs::create_dir(dir.path().join("new")).unwrap();
        let (collection, warning) = load_collection("remote", &settings, dir.path());
        assert!(warning.is_none());
        assert_eq!(collection.calendars.len(), 2);
    }

    #[test]
    fn snapshot_groups_collections_in_id_order_without_using_visibility_filters() {
        let dir = tempfile::tempdir().unwrap();
        let xdg = std::sync::Arc::new(eventix_state::with_test_xdg(dir.path(), dir.path()));
        let mut state = eventix_state::State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        for id in ["z", "a"] {
            let mut collection = CollectionSettings::new(SyncerType::FileSystem {
                path: dir.path().to_string_lossy().into_owned(),
            });
            let mut calendar = CalendarSettings::default();
            calendar.set_name("Same name".into());
            calendar.set_enabled(id == "z");
            collection.all_calendars_mut().insert(id.into(), calendar);
            state
                .settings_mut()
                .collections_mut()
                .insert(id.into(), collection);
        }
        state.misc_mut().toggle_calendar(&"z".into());
        let snapshot = Snapshot::load(&state);
        assert!(snapshot.warnings.is_empty());
        assert_eq!(
            snapshot
                .collections
                .iter()
                .map(|col| col.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert!(!snapshot.collections[0].calendars[0].enabled);
        assert!(snapshot.collections[1].calendars[0].enabled);
    }
}
