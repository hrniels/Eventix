// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Context;
use once_cell::sync::Lazy;
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use toml_edit::{DocumentMut, value};

type MigrationFn = fn(&mut DocumentMut, Option<&DocumentMut>);

struct Migration {
    version: u32,
    filename: &'static str,
    migrate: MigrationFn,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 0,
        filename: "settings.toml",
        migrate: migrate_settings_v0_to_v1,
    },
    Migration {
        version: 1,
        filename: "settings.toml",
        migrate: migrate_settings_v1_to_v2,
    },
    Migration {
        version: 2,
        filename: "misc.toml",
        migrate: migrate_misc_v2_to_v3,
    },
    Migration {
        version: 3,
        filename: "misc.toml",
        migrate: migrate_misc_v3_to_v4,
    },
];

pub fn migrate_if_needed(path: &PathBuf) -> anyhow::Result<()> {
    migrate_with_settings(path, None)
}

/// Migrates miscellaneous state using calendar ownership from the configured settings file.
pub(crate) fn migrate_misc_if_needed(xdg: &xdg::BaseDirectories) -> anyhow::Result<()> {
    let Some(path) = xdg.find_data_file("misc.toml") else {
        return Ok(());
    };
    migrate_with_settings(&path, xdg.find_config_file("settings.toml").as_ref())
}

fn migrate_with_settings(path: &PathBuf, settings_path: Option<&PathBuf>) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let filename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default();
    // ignore previously created backup files (this is okay, because although alarms are stored as
    // <cal-id>.toml, the calendar id is a generated uuid).
    static BACKUP_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\.v\d+\.toml$").unwrap());
    if BACKUP_RE.is_match(filename) {
        return Ok(());
    }

    let content = fs::read_to_string(path).context("reading toml for migration")?;
    let mut doc = content
        .parse::<DocumentMut>()
        .context("parsing toml for migration")?;

    let mut version = doc.get("version").and_then(|v| v.as_integer()).unwrap_or(0) as u32;

    if version >= crate::CURRENT_VERSION {
        return Ok(());
    }

    let settings = settings_path
        .map(|path| -> anyhow::Result<DocumentMut> {
            fs::read_to_string(path)
                .context("reading settings for migration")?
                .parse::<DocumentMut>()
                .context("parsing settings for migration")
        })
        .transpose()?;

    // Create backup of the original version
    let mut backup_path = path.clone();
    backup_path.set_extension(format!("v{}.toml", version));
    fs::copy(path, &backup_path).context("creating backup before migration")?;

    // Perform migrations in a loop until we reach CURRENT_VERSION
    while version < crate::CURRENT_VERSION {
        for m in MIGRATIONS {
            if m.version == version {
                let matches = match m.filename {
                    "alarms" => {
                        filename.ends_with(".toml")
                            && filename != "settings.toml"
                            && filename != "misc.toml"
                    }
                    f => f == filename,
                };

                if matches {
                    (m.migrate)(&mut doc, settings.as_ref());
                }
            }
        }

        // Increment version. Even if no specific migration was found (e.g. for files that
        // don't need changes in this version), we still advance to the next version.
        version += 1;
        doc.insert("version", value(version as i64));
    }

    // Write back
    fs::write(path, doc.to_string()).context("writing migrated toml")?;

    Ok(())
}

fn migrate_settings_v0_to_v1(doc: &mut DocumentMut, _settings: Option<&DocumentMut>) {
    if let Some(collections) = doc.get_mut("collection").and_then(|v| v.as_table_mut()) {
        for (_, col) in collections.iter_mut() {
            if let Some(syncer) = col.get_mut("syncer").and_then(|v| v.as_table_mut()) {
                for (_, ty) in syncer.iter_mut() {
                    if let Some(ty_table) = ty.as_table_mut()
                        && let Some(pw_cmd) = ty_table.remove("password_cmd")
                    {
                        let mut pw_source = toml_edit::Table::new();
                        pw_source.insert("type", value("Command"));
                        pw_source.insert("command", pw_cmd);
                        ty_table.insert("password_source", toml_edit::Item::Table(pw_source));
                    }
                }
            }
        }
    }
}

fn migrate_settings_v1_to_v2(doc: &mut DocumentMut, _settings: Option<&DocumentMut>) {
    if let Some(collections) = doc.get_mut("collection").and_then(|v| v.as_table_mut()) {
        for (_, col) in collections.iter_mut() {
            if let Some(syncer) = col.get_mut("syncer").and_then(|v| v.as_table_mut()) {
                for (_, ty) in syncer.iter_mut() {
                    if let Some(ty_table) = ty.as_table_mut() {
                        // Both VDirSyncer and O365 use encrypted-password now.
                        // Legacy generic password_source was used by both.
                        if ty_table.remove("password_source").is_some() {
                            // CalDAV (VDirSyncer) gets an empty encrypted-password field.
                            // O365 also gets one if it's missing or if the generic source was removed.
                            let mut pw = toml_edit::InlineTable::new();
                            pw.insert("nonce", "".into());
                            pw.insert("ciphertext", "".into());
                            ty_table.insert(
                                "password",
                                toml_edit::Item::Value(toml_edit::Value::InlineTable(pw)),
                            );
                        }
                    }
                }
            }
        }
    }
}

fn migrate_misc_v2_to_v3(doc: &mut DocumentMut, _settings: Option<&DocumentMut>) {
    doc.remove("collection_tokens");
}

fn migrate_misc_v3_to_v4(doc: &mut DocumentMut, settings: Option<&DocumentMut>) {
    let failed_calendars = doc.remove("calendar_errors");
    let errors = doc
        .entry("collection_errors")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let Some(failed_calendars) = failed_calendars.as_ref().and_then(|item| item.as_array()) else {
        return;
    };
    let Some(collections) = settings
        .and_then(|settings| settings.get("collection"))
        .and_then(|item| item.as_table_like())
    else {
        return;
    };

    // Legacy flags have no message; an empty message preserves failure without fixing its locale.
    for (id, collection) in collections.iter() {
        if let Some(calendars) = collection
            .get("calendar")
            .and_then(|item| item.as_table_like())
            && failed_calendars
                .iter()
                .filter_map(|value| value.as_str())
                .any(|calendar| calendars.contains_key(calendar))
            && errors.get(id).is_none()
        {
            errors[id] = value("");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_migrate_v0_to_v1_settings() {
        let dir = tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");

        let v0_content = r#"
[collection.work]
syncer.VDirSyncer.email.name = "Alice"
syncer.VDirSyncer.email.address = "alice@example.com"
syncer.VDirSyncer.url = "https://dav.example.com"
syncer.VDirSyncer.read_only = false
syncer.VDirSyncer.username = "alice"
syncer.VDirSyncer.password_cmd = ["pass", "show", "work"]

[collection.home]
syncer.FileSystem.path = "/data"
"#;
        fs::write(&settings_path, v0_content).unwrap();

        migrate_if_needed(&settings_path).unwrap();

        let migrated_content = fs::read_to_string(&settings_path).unwrap();
        assert!(migrated_content.contains(&format!("version = {}", crate::CURRENT_VERSION)));
        // Check that it passed through v1 (Command type) and ended in v2 (empty encrypted password)
        assert!(!migrated_content.contains("password_cmd"));
        assert!(!migrated_content.contains("password_source"));
        assert!(migrated_content.contains("password = { nonce = \"\", ciphertext = \"\" }"));

        // Check backup
        let backup_path = dir.path().join("settings.v0.toml");
        assert!(backup_path.exists());
        assert_eq!(fs::read_to_string(backup_path).unwrap(), v0_content);
    }

    #[test]
    fn test_migrate_current_version_no_op() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("misc.toml");

        let current_content = format!(
            "version = {}\nlocale_type = \"English\"\n",
            crate::CURRENT_VERSION
        );
        fs::write(&path, &current_content).unwrap();

        migrate_if_needed(&path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, current_content);

        // No backup should be created if no migration was performed
        let backup_path = dir
            .path()
            .join(format!("misc.v{}.toml", crate::CURRENT_VERSION));
        assert!(!backup_path.exists());
    }

    #[test]
    fn test_migrate_misc_v2_to_v3_removes_collection_tokens() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("misc.toml");
        let v2_content = r#"version = 2
locale_type = "German"
disabled_calendars = ["cal1"]

[collection_tokens]
o365 = "refresh-token"
"#;
        fs::write(&path, v2_content).unwrap();

        migrate_if_needed(&path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains(&format!("version = {}", crate::CURRENT_VERSION)));
        assert!(content.contains("locale_type = \"German\""));
        assert!(content.contains("disabled_calendars = [\"cal1\"]"));
        assert!(!content.contains("collection_tokens"));
        assert_eq!(
            fs::read_to_string(dir.path().join("misc.v2.toml")).unwrap(),
            v2_content
        );
    }

    #[test]
    fn migrate_misc_v3_to_v4_preserves_failure_flags_and_tokens() {
        let dir = tempdir().unwrap();
        let config = tempdir().unwrap();
        let xdg = crate::with_test_xdg(dir.path(), config.path());
        let path = xdg.place_data_file("misc.toml").unwrap();
        let original = "version = 3\ncalendar_errors = [\"cal1\", \"cal2\", \"cal3\", \"deleted\"]\n\
            disabled_calendars = [\"cal2\"]\n[collection_tokens]\nwork = \"token\"\n\
            [collection_errors]\nexisting = \"Connection refused\"\n";
        fs::write(&path, original).unwrap();
        let settings_path = xdg.place_config_file("settings.toml").unwrap();
        let mut settings = crate::Settings::new(settings_path.clone());
        for (id, calendars) in [
            ("work", vec!["cal1", "cal2"]),
            ("home", vec!["healthy"]),
            ("existing", vec!["cal3"]),
        ] {
            let mut collection = crate::CollectionSettings::new(crate::SyncerType::FileSystem {
                path: dir.path().display().to_string(),
            });
            for id in calendars {
                collection
                    .all_calendars_mut()
                    .insert(id.into(), crate::CalendarSettings::default());
            }
            settings.collections_mut().insert(id.into(), collection);
        }
        let mut settings_doc = toml::to_string(&settings)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        settings_doc["version"] = value(3);
        settings_doc.remove("preferences");
        fs::write(&settings_path, settings_doc.to_string()).unwrap();

        // Settings may read misc first to import legacy preferences; that must not lose errors.
        crate::Settings::load_from_file(&xdg).unwrap();

        let misc = crate::Misc::load_from_file(&xdg).unwrap();
        assert!(misc.calendar_disabled(&"cal2".into()));
        assert_eq!(misc.collection_error("work"), Some(""));
        assert_eq!(misc.collection_error("home"), None);
        assert_eq!(
            misc.collection_error("existing"),
            Some("Connection refused")
        );
        assert_eq!(misc.collection_token(&"work".into()).unwrap(), "token");
        let doc = fs::read_to_string(&path)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(doc["version"].as_integer(), Some(4));
        assert!(doc.get("calendar_errors").is_none());
        assert_eq!(doc["collection_errors"].as_table().unwrap().len(), 2);
        assert_eq!(
            fs::read_to_string(dir.path().join("misc.v3.toml")).unwrap(),
            original
        );
        let serialized = toml::to_string(&misc).unwrap();
        assert!(!serialized.contains("calendar_errors"));
        // A second load must preserve migrated generic failures and existing messages.
        let loaded = crate::Misc::load_from_file(&xdg).unwrap();
        assert_eq!(loaded, misc);
    }

    #[test]
    fn test_migrate_v1_to_v2_settings() {
        let dir = tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");

        let v1_content = r#"version = 1
[collection.caldav]
syncer.VDirSyncer.email.name = "Alice"
syncer.VDirSyncer.email.address = "alice@example.com"
syncer.VDirSyncer.url = "https://dav.example.com"
syncer.VDirSyncer.read_only = false
syncer.VDirSyncer.username = "alice"
syncer.VDirSyncer.password_source.type = "Command"
syncer.VDirSyncer.password_source.command = ["pass", "show", "work"]

[collection.o365]
syncer.O365.email.name = "Bob"
syncer.O365.email.address = "bob@example.com"
syncer.O365.username = "bob"
syncer.O365.read_only = false
"#;
        fs::write(&settings_path, v1_content).unwrap();

        migrate_if_needed(&settings_path).unwrap();

        let migrated_content = fs::read_to_string(&settings_path).unwrap();
        assert!(migrated_content.contains(&format!("version = {}", crate::CURRENT_VERSION)));
        // CalDAV check: had password_source, so should have password
        assert!(!migrated_content.contains("password_source"));
        assert!(migrated_content.contains("password = { nonce = \"\", ciphertext = \"\" }"));
        // O365 check: had NO password_source, so should have NO password
        assert!(migrated_content.contains("[collection.o365]"));
        assert!(
            !migrated_content
                .contains("syncer.O365.password = { nonce = \"\", ciphertext = \"\" }")
        );

        // Check backups
        let backup_v1 = dir.path().join("settings.v1.toml");
        assert!(backup_v1.exists());
    }

    #[test]
    fn test_migrate_v1_to_v2_no_password_source() {
        let dir = tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");

        let v1_content = r#"version = 1
[collection.caldav]
syncer.VDirSyncer.email.name = "Alice"
syncer.VDirSyncer.email.address = "alice@example.com"
syncer.VDirSyncer.url = "https://dav.example.com"
syncer.VDirSyncer.read_only = true
syncer.VDirSyncer.username = "alice"

[collection.local]
syncer.FileSystem.path = "/data"
"#;
        fs::write(&settings_path, v1_content).unwrap();

        migrate_if_needed(&settings_path).unwrap();

        let migrated_content = fs::read_to_string(&settings_path).unwrap();
        assert!(migrated_content.contains(&format!("version = {}", crate::CURRENT_VERSION)));
        // VDirSyncer check: had NO password_source, so should have NO password
        assert!(!migrated_content.contains("syncer.VDirSyncer.password_source"));
        assert!(!migrated_content.contains("syncer.VDirSyncer.password ="));

        // FileSystem check: never has a password, should still not have one
        assert!(migrated_content.contains("[collection.local]"));
        assert!(!migrated_content.contains("syncer.FileSystem.password ="));
    }

    #[test]
    fn test_migrate_v0_to_v2_settings() {
        let dir = tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");

        let v0_content = r#"
[collection.work]
syncer.VDirSyncer.email.name = "Alice"
syncer.VDirSyncer.email.address = "alice@example.com"
syncer.VDirSyncer.url = "https://dav.example.com"
syncer.VDirSyncer.read_only = false
syncer.VDirSyncer.username = "alice"
syncer.VDirSyncer.password_cmd = ["pass", "show", "work"]
"#;
        fs::write(&settings_path, v0_content).unwrap();

        migrate_if_needed(&settings_path).unwrap();

        let migrated_content = fs::read_to_string(&settings_path).unwrap();
        assert!(migrated_content.contains(&format!("version = {}", crate::CURRENT_VERSION)));
        assert!(!migrated_content.contains("password_cmd"));
        assert!(!migrated_content.contains("password_source"));
        assert!(migrated_content.contains("password = { nonce = \"\", ciphertext = \"\" }"));

        // Check backups (it should have created a v0 backup first)
        let backup_v0 = dir.path().join("settings.v0.toml");
        assert!(backup_v0.exists());
    }
}
