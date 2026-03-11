// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for [`CalDir`] filesystem operations.
//!
//! These tests exercise methods that interact with the real filesystem. Each test gets its own
//! temporary directory so tests are fully isolated and leave no artifacts behind.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use eventix_ical::col::{CalDir, ColError};
use eventix_ical::objects::{EventLike, UpdatableEventLike};

mod common;
use common::{copy_fixture, make_id};

// ---------------------------------------------------------------------------
// new_from_dir
// ---------------------------------------------------------------------------

#[test]
fn new_from_dir_loads_ics_files() {
    let tmp = TempDir::new().unwrap();
    copy_fixture("event_a.ics", &tmp);
    copy_fixture("event_b.ics", &tmp);

    let dir =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();

    assert_eq!(dir.files().len(), 2);
    assert!(dir.file_by_id("event-a").is_some());
    assert!(dir.file_by_id("event-b").is_some());
}

#[test]
fn new_from_dir_empty_dir() {
    let tmp = TempDir::new().unwrap();

    let dir =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();

    assert!(dir.files().is_empty());
}

#[test]
fn new_from_dir_skips_non_ics_and_subdirectories() {
    let tmp = TempDir::new().unwrap();
    copy_fixture("event_a.ics", &tmp);

    // A plain text file must be ignored.
    fs::write(tmp.path().join("notes.txt"), "not a calendar").unwrap();
    // A file with no extension must be ignored.
    fs::write(tmp.path().join("README"), "no extension").unwrap();
    // A subdirectory must be ignored (not cause an error either).
    fs::create_dir(tmp.path().join("subdir")).unwrap();

    let dir =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();

    assert_eq!(dir.files().len(), 1);
    assert!(dir.file_by_id("event-a").is_some());
}

#[test]
fn new_from_dir_nonexistent_path_returns_error() {
    let result = CalDir::new_from_dir(
        make_id("cal"),
        PathBuf::from("/nonexistent/path/that/does/not/exist"),
        "Test".into(),
    );

    assert!(matches!(result, Err(ColError::ReadDir(_, _))));
}

// ---------------------------------------------------------------------------
// rescan_for_additions
// ---------------------------------------------------------------------------

#[test]
fn rescan_for_additions_detects_new_file() {
    let tmp = TempDir::new().unwrap();
    copy_fixture("event_a.ics", &tmp);

    let mut dir =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();

    assert_eq!(dir.files().len(), 1);

    let changed = dir.rescan_for_additions().unwrap();
    assert!(!changed);
    assert_eq!(dir.files().len(), 1);

    // Add a second file to the directory after the initial load.
    copy_fixture("event_b.ics", &tmp);

    let changed = dir.rescan_for_additions().unwrap();

    assert!(changed);
    assert_eq!(dir.files().len(), 2);
    assert!(dir.file_by_id("event-b").is_some());
}

// ---------------------------------------------------------------------------
// rescan_files
// ---------------------------------------------------------------------------

#[test]
fn rescan_files_reloads_changed_file() {
    let tmp = TempDir::new().unwrap();
    copy_fixture("event_a.ics", &tmp);

    let mut dir =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();

    // Verify original summary.
    let summary_before = dir
        .files()
        .first()
        .and_then(|f| f.components().first())
        .and_then(|c| c.summary().cloned());
    assert_eq!(summary_before.as_deref(), Some("Event A"));

    // Overwrite the file on disk with a modified SUMMARY.
    fs::write(
        tmp.path().join("event_a.ics"),
        concat!(
            "BEGIN:VCALENDAR\r\n",
            "VERSION:2.0\r\n",
            "PRODID:-//Test//Test//EN\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:event-a\r\n",
            "DTSTART;VALUE=DATE:20240101\r\n",
            "DTEND;VALUE=DATE:20240102\r\n",
            "SUMMARY:Updated A\r\n",
            "END:VEVENT\r\n",
            "END:VCALENDAR\r\n",
        ),
    )
    .unwrap();

    let changed = dir.rescan_files().unwrap();

    assert!(changed);

    let summary_after = dir
        .files()
        .first()
        .and_then(|f| f.components().first())
        .and_then(|c| c.summary().cloned());
    assert_eq!(summary_after.as_deref(), Some("Updated A"));
}

// ---------------------------------------------------------------------------
// rescan_for_deletions
// ---------------------------------------------------------------------------

#[test]
fn rescan_for_deletions_detects_removed_file() {
    let tmp = TempDir::new().unwrap();
    copy_fixture("event_a.ics", &tmp);
    copy_fixture("event_b.ics", &tmp);

    let mut dir =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();

    assert_eq!(dir.files().len(), 2);

    let changed = dir.rescan_for_deletions();
    assert!(!changed);
    assert_eq!(dir.files().len(), 2);

    // Remove one file from disk.
    fs::remove_file(tmp.path().join("event_b.ics")).unwrap();

    let changed = dir.rescan_for_deletions();

    assert!(changed);
    assert_eq!(dir.files().len(), 1);
    assert!(dir.file_by_id("event-b").is_none());
}

// ---------------------------------------------------------------------------
// delete_by_uid / remove_file
// ---------------------------------------------------------------------------

#[test]
fn delete_by_uid_removes_component_and_file_from_disk() {
    let tmp = TempDir::new().unwrap();
    let path = copy_fixture("event_a.ics", &tmp);

    let mut dir =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();

    assert!(path.exists());

    dir.delete_by_uid("event-a").unwrap();

    // The file is gone from disk.
    assert!(!path.exists());
    // The collection is empty.
    assert!(dir.files().is_empty());
}

#[test]
fn delete_by_uid_saves_file_when_not_empty() {
    // A file containing two distinct UIDs: after deleting one, the file must be saved (not
    // removed) and the remaining component must still be present on disk.
    let tmp = TempDir::new().unwrap();
    let ics_path = copy_fixture("two_events.ics", &tmp);

    let mut dir =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();

    assert_eq!(dir.files().len(), 1);

    // Delete event-c; event-d should remain.
    dir.delete_by_uid("event-c").unwrap();

    // The file is still present on disk (because event-d remains).
    assert!(ics_path.exists());

    // Reload and verify only event-d survives.
    let reloaded =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();
    assert!(reloaded.file_by_id("event-c").is_none());
    assert!(reloaded.file_by_id("event-d").is_some());
}

#[test]
fn delete_by_uid_not_found_returns_error() {
    let tmp = TempDir::new().unwrap();
    copy_fixture("event_a.ics", &tmp);

    let mut dir =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();

    let result = dir.delete_by_uid("uid-does-not-exist");
    assert!(matches!(result, Err(ColError::ComponentNotFound(_))));
}

// ---------------------------------------------------------------------------
// save
// ---------------------------------------------------------------------------

#[test]
fn save_persists_mutations_to_disk() {
    let tmp = TempDir::new().unwrap();
    copy_fixture("event_a.ics", &tmp);

    let mut dir =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();

    // Mutate the summary of event-a in memory.
    let file = dir.file_by_id_mut("event-a").unwrap();
    let comp = file
        .component_with_mut(|c| c.uid() == "event-a" && c.rid().is_none())
        .unwrap();
    comp.set_summary(Some("Saved A".into()));

    // Persist.
    dir.save().unwrap();

    // Reload the directory from disk and verify the mutation survived.
    let reloaded =
        CalDir::new_from_dir(make_id("cal"), tmp.path().to_path_buf(), "Test".into()).unwrap();
    let summary = reloaded
        .files()
        .first()
        .and_then(|f| f.components().first())
        .and_then(|c| c.summary().cloned());
    assert_eq!(summary.as_deref(), Some("Saved A"));
}
