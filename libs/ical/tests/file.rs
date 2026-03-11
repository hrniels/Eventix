// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for [`CalFile`] filesystem operations.
//!
//! These tests exercise methods that interact with the real filesystem. Each test gets its own
//! temporary directory so tests are fully isolated and leave no artifacts behind.

use std::path::PathBuf;

use tempfile::TempDir;

use eventix_ical::col::{CalFile, ColError};
use eventix_ical::objects::{CalComponent, CalEvent, Calendar, EventLike, UpdatableEventLike};

mod common;
use common::{copy_fixture, data_dir, make_id};

// ---------------------------------------------------------------------------
// new_from_external_file
// ---------------------------------------------------------------------------

#[test]
fn new_from_external_file_splits_by_uid() {
    // multi_uid.ics contains two VEVENTs with different UIDs in a single VCALENDAR.  The method
    // must split them into two CalFile instances, each stored at <uid>.ics inside dir_path.
    let tmp = TempDir::new().unwrap();
    let src = data_dir().join("multi_uid.ics");

    let files =
        CalFile::new_from_external_file(make_id("cal"), tmp.path().to_path_buf(), src).unwrap();

    assert_eq!(files.len(), 2);

    let uids: Vec<_> = files
        .iter()
        .flat_map(|f| f.components().iter().map(|c| c.uid().clone()))
        .collect();
    assert!(uids.contains(&"multi-uid-alpha".to_string()));
    assert!(uids.contains(&"multi-uid-beta".to_string()));

    // Each file's path should be derived from its UID inside dir_path.
    for file in &files {
        let expected_name = format!("{}.ics", file.components()[0].uid());
        assert_eq!(
            file.path().file_name().unwrap().to_str().unwrap(),
            expected_name
        );
        assert_eq!(file.path().parent().unwrap(), tmp.path());
    }
}

#[test]
fn new_from_external_file_nonexistent_path_returns_error() {
    let tmp = TempDir::new().unwrap();
    let result = CalFile::new_from_external_file(
        make_id("cal"),
        tmp.path().to_path_buf(),
        PathBuf::from("/nonexistent/file.ics"),
    );
    assert!(matches!(result, Err(ColError::FileOpen(_, _))));
}

#[test]
fn new_from_external_file_parse_error_returns_error() {
    let tmp = TempDir::new().unwrap();
    let src = data_dir().join("invalid.ics");
    let result = CalFile::new_from_external_file(make_id("cal"), tmp.path().to_path_buf(), src);
    assert!(matches!(result, Err(ColError::FileParse(_, _))));
}

// ---------------------------------------------------------------------------
// new_from_file error paths
// ---------------------------------------------------------------------------

#[test]
fn new_from_file_nonexistent_path_returns_error() {
    let result = CalFile::new_from_file(make_id("cal"), PathBuf::from("/nonexistent/missing.ics"));
    assert!(matches!(result, Err(ColError::FileOpen(_, _))));
}

#[test]
fn new_from_file_parse_error_returns_error() {
    let result = CalFile::new_from_file(make_id("cal"), data_dir().join("invalid.ics"));
    assert!(matches!(result, Err(ColError::FileParse(_, _))));
}

// ---------------------------------------------------------------------------
// last_modified
// ---------------------------------------------------------------------------

#[test]
fn last_modified_returns_time_for_real_file() {
    let tmp = TempDir::new().unwrap();
    let path = copy_fixture("event_a.ics", &tmp);
    let file = CalFile::new_from_file(make_id("cal"), path).unwrap();

    assert!(file.last_modified().is_ok());
}

#[test]
fn last_modified_error_on_nonexistent_file() {
    // Construct a CalFile that points to a path that does not exist on disk.
    let cal: Calendar = concat!(
        "BEGIN:VCALENDAR\r\n",
        "VERSION:2.0\r\n",
        "PRODID:-//Test//Test//EN\r\n",
        "BEGIN:VEVENT\r\n",
        "UID:ghost\r\n",
        "DTSTART;VALUE=DATE:20240101\r\n",
        "DTEND;VALUE=DATE:20240102\r\n",
        "DTSTAMP:20240101T000000Z\r\n",
        "END:VEVENT\r\n",
        "END:VCALENDAR\r\n",
    )
    .parse()
    .unwrap();
    let file = CalFile::new(make_id("cal"), PathBuf::from("/does/not/exist.ics"), cal);

    assert!(matches!(
        file.last_modified(),
        Err(ColError::FileMetadata(_))
    ));
}

// ---------------------------------------------------------------------------
// save / reload_calendar round-trip
// ---------------------------------------------------------------------------

#[test]
fn save_creates_file_on_disk() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("new_event.ics");

    let mut event = CalEvent::new("saved-uid");
    event.set_summary(Some("Saved Summary".into()));
    let mut cal = Calendar::default();
    cal.add_component(CalComponent::Event(event));

    let file = CalFile::new(make_id("cal"), path.clone(), cal);
    file.save().unwrap();

    assert!(path.exists());
}

#[test]
fn save_and_reload_calendar_round_trip() {
    // Write a CalFile to disk, reload it, and verify the mutation survived.
    let tmp = TempDir::new().unwrap();
    let path = copy_fixture("event_a.ics", &tmp);
    let mut file = CalFile::new_from_file(make_id("cal"), path).unwrap();

    // Mutate the summary in memory.
    file.component_with_mut(|c| c.uid() == "event-a")
        .unwrap()
        .set_summary(Some("Reloaded".into()));

    // Persist to disk.
    file.save().unwrap();

    // Reload from disk and verify the mutation survived.
    file.reload_calendar().unwrap();
    let summary = file
        .component_with(|c| c.uid() == "event-a")
        .and_then(|c| c.summary().cloned());
    assert_eq!(summary.as_deref(), Some("Reloaded"));
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

#[test]
fn remove_deletes_file_from_disk() {
    let tmp = TempDir::new().unwrap();
    let path = copy_fixture("event_a.ics", &tmp);
    assert!(path.exists());

    let mut file = CalFile::new_from_file(make_id("cal"), path.clone()).unwrap();
    file.remove().unwrap();

    assert!(!path.exists());
}
