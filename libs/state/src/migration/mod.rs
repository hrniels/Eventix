// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Context;
use once_cell::sync::Lazy;
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use toml_edit::{DocumentMut, value};

type MigrationFn = fn(&mut DocumentMut);

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
];

pub fn migrate_if_needed(path: &PathBuf) -> anyhow::Result<()> {
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
                    (m.migrate)(&mut doc);
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

fn migrate_settings_v0_to_v1(doc: &mut DocumentMut) {
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

fn migrate_settings_v1_to_v2(doc: &mut DocumentMut) {
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

fn migrate_misc_v2_to_v3(doc: &mut DocumentMut) {
    doc.remove("collection_tokens");
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
        assert!(migrated_content.contains("version = 3"));
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

        let v3_content = r#"version = 3
locale_type = "English"
"#;
        fs::write(&path, v3_content).unwrap();

        migrate_if_needed(&path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, v3_content);

        // No backup should be created if no migration was performed
        let backup_path = dir.path().join("misc.v3.toml");
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
        assert!(content.contains("version = 3"));
        assert!(content.contains("locale_type = \"German\""));
        assert!(content.contains("disabled_calendars = [\"cal1\"]"));
        assert!(!content.contains("collection_tokens"));
        assert_eq!(
            fs::read_to_string(dir.path().join("misc.v2.toml")).unwrap(),
            v2_content
        );
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
        assert!(migrated_content.contains("version = 3"));
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
        assert!(migrated_content.contains("version = 3"));
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
        assert!(migrated_content.contains("version = 3"));
        assert!(!migrated_content.contains("password_cmd"));
        assert!(!migrated_content.contains("password_source"));
        assert!(migrated_content.contains("password = { nonce = \"\", ciphertext = \"\" }"));

        // Check backups (it should have created a v0 backup first)
        let backup_v0 = dir.path().join("settings.v0.toml");
        assert!(backup_v0.exists());
    }
}
