// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for [`CalStore`] filesystem operations.
//!
//! These tests exercise the full public API of [`CalStore`] including methods that delegate to
//! [`CalDir`] and [`CalFile`] and interact with the real filesystem. Each test gets its own
//! temporary directory so tests are fully isolated and leave no artifacts behind.

use std::path::PathBuf;

use chrono::TimeZone;
use chrono_tz::{Tz, UTC};
use tempfile::TempDir;

use eventix_ical::col::{CalDir, CalStore, ColError};
use eventix_ical::objects::{DefaultAlarmOverlay, EventLike, UpdatableEventLike};

mod common;
use common::{copy_fixture, make_id};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn utc(year: i32, month: u32, day: u32, h: u32, m: u32, s: u32) -> chrono::DateTime<chrono_tz::Tz> {
    UTC.with_ymd_and_hms(year, month, day, h, m, s).unwrap()
}

/// Builds a [`CalStore`] from two separate temporary directories, each containing the
/// fixture files listed in `fixtures_a` and `fixtures_b` respectively.
fn two_dir_store(
    tmp_a: &TempDir,
    fixtures_a: &[&str],
    tmp_b: &TempDir,
    fixtures_b: &[&str],
) -> CalStore {
    for f in fixtures_a {
        copy_fixture(f, tmp_a);
    }
    for f in fixtures_b {
        copy_fixture(f, tmp_b);
    }

    let dir_a = CalDir::new_from_dir(
        make_id("dir-a"),
        tmp_a.path().to_path_buf(),
        "DirA".into(),
        &Tz::UTC,
    )
    .unwrap();
    let dir_b = CalDir::new_from_dir(
        make_id("dir-b"),
        tmp_b.path().to_path_buf(),
        "DirB".into(),
        &Tz::UTC,
    )
    .unwrap();

    let mut store = CalStore::default();
    store.add(dir_a);
    store.add(dir_b);
    store
}

// ---------------------------------------------------------------------------
// occurrences_between
// ---------------------------------------------------------------------------

#[test]
fn occurrences_between_spans_multiple_dirs() {
    let tmp_a = TempDir::new().unwrap();
    let tmp_b = TempDir::new().unwrap();
    let store = two_dir_store(&tmp_a, &["event_a.ics"], &tmp_b, &["event_b.ics"]);

    // event-a is on 2024-01-01, event-b is on 2024-06-01; a wide window captures both.
    let occs: Vec<_> = store
        .occurrences_between(
            utc(2024, 1, 1, 0, 0, 0),
            utc(2024, 12, 31, 23, 59, 59),
            |_| true,
        )
        .collect();

    let uids: Vec<_> = occs.iter().map(|o| o.uid().clone()).collect();
    assert!(uids.contains(&"event-a".to_string()), "event-a missing");
    assert!(uids.contains(&"event-b".to_string()), "event-b missing");
}

#[test]
fn occurrences_between_filter_is_applied() {
    let tmp_a = TempDir::new().unwrap();
    let tmp_b = TempDir::new().unwrap();
    // event_with_todo.ics contains a VTODO, event_a.ics a VEVENT.
    let store = two_dir_store(&tmp_a, &["event_a.ics"], &tmp_b, &["event_with_todo.ics"]);

    // Filter to only events (not TODOs).
    use eventix_ical::objects::CalCompType;
    let occs: Vec<_> = store
        .occurrences_between(
            utc(2024, 1, 1, 0, 0, 0),
            utc(2024, 12, 31, 23, 59, 59),
            |c| c.ctype() == CalCompType::Event,
        )
        .collect();

    assert_eq!(occs.len(), 1);
    assert_eq!(occs[0].uid().as_str(), "event-a");
}

// ---------------------------------------------------------------------------
// occurrence_by_id
// ---------------------------------------------------------------------------

#[test]
fn occurrence_by_id_base_component() {
    let tmp_a = TempDir::new().unwrap();
    let tmp_b = TempDir::new().unwrap();
    let store = two_dir_store(&tmp_a, &["event_a.ics"], &tmp_b, &["event_b.ics"]);

    let occ = store.occurrence_by_id("event-a", None, &UTC);
    assert!(occ.is_some(), "expected occurrence for event-a");
    assert_eq!(occ.unwrap().uid().as_str(), "event-a");
}

#[test]
fn occurrence_by_id_missing_uid_returns_none() {
    let tmp_a = TempDir::new().unwrap();
    let tmp_b = TempDir::new().unwrap();
    let store = two_dir_store(&tmp_a, &["event_a.ics"], &tmp_b, &["event_b.ics"]);

    assert!(store.occurrence_by_id("no-such-uid", None, &UTC).is_none());
}

#[test]
fn occurrence_by_id_with_rid_returns_overwrite() {
    // recurring_with_overwrite.ics has a weekly recurrence starting 2024-01-01,
    // with an overwrite for the second occurrence (rid = 2024-01-08).
    let tmp = TempDir::new().unwrap();
    copy_fixture("recurring_with_overwrite.ics", &tmp);
    let dir = CalDir::new_from_dir(
        make_id("dir"),
        tmp.path().to_path_buf(),
        "D".into(),
        &Tz::UTC,
    )
    .unwrap();
    let mut store = CalStore::default();
    store.add(dir);

    use chrono::NaiveDate;
    use eventix_ical::objects::CalDate;

    // Look up the overwritten occurrence via its rid.
    let rid = CalDate::Date(
        NaiveDate::from_ymd_opt(2024, 1, 8).unwrap(),
        eventix_ical::objects::CalCompType::Event.into(),
    );
    let occ = store.occurrence_by_id("recurring-ow", Some(&rid), &UTC);
    assert!(
        occ.is_some(),
        "expected occurrence for recurring-ow with rid"
    );
    // The overwrite changes the summary to "Overwritten Second".
    let occ = occ.unwrap();
    assert_eq!(
        occ.summary().map(String::as_str),
        Some("Overwritten Second")
    );
}

// ---------------------------------------------------------------------------
// due_alarms_between
// ---------------------------------------------------------------------------

#[test]
fn due_alarms_between_delegates_to_dirs() {
    // event_with_alarm.ics contains an event starting at 2024-03-01T10:00Z with a -PT15M alarm,
    // so the alarm fires at 2024-03-01T09:45Z.
    let tmp_a = TempDir::new().unwrap();
    let tmp_b = TempDir::new().unwrap();
    let store = two_dir_store(&tmp_a, &["event_with_alarm.ics"], &tmp_b, &["event_a.ics"]);

    let overlay = DefaultAlarmOverlay;

    // Window that contains the alarm time.
    let alarms: Vec<_> = store
        .due_alarms_between(
            utc(2024, 3, 1, 9, 0, 0),
            utc(2024, 3, 1, 10, 0, 0),
            &overlay,
        )
        .collect();

    assert_eq!(alarms.len(), 1);
    assert_eq!(alarms[0].occurrence().uid().as_str(), "event-alarm");

    // Window that does not contain the alarm time yields nothing.
    let alarms_empty: Vec<_> = store
        .due_alarms_between(
            utc(2024, 3, 1, 10, 0, 0),
            utc(2024, 3, 1, 11, 0, 0),
            &overlay,
        )
        .collect();
    assert!(alarms_empty.is_empty());
}

// ---------------------------------------------------------------------------
// save
// ---------------------------------------------------------------------------

#[test]
fn save_persists_all_dirs() {
    let tmp_a = TempDir::new().unwrap();
    let tmp_b = TempDir::new().unwrap();
    copy_fixture("event_a.ics", &tmp_a);
    copy_fixture("event_b.ics", &tmp_b);

    let dir_a = CalDir::new_from_dir(
        make_id("dir-a"),
        tmp_a.path().to_path_buf(),
        "DirA".into(),
        &Tz::UTC,
    )
    .unwrap();
    let dir_b = CalDir::new_from_dir(
        make_id("dir-b"),
        tmp_b.path().to_path_buf(),
        "DirB".into(),
        &Tz::UTC,
    )
    .unwrap();

    let mut store = CalStore::default();
    store.add(dir_a);
    store.add(dir_b);

    // Mutate one event in each directory.
    store
        .try_files_by_id_mut("event-a")
        .unwrap()
        .component_with_mut(|c| c.uid() == "event-a")
        .unwrap()
        .set_summary(Some("Saved A".into()));

    store
        .try_files_by_id_mut("event-b")
        .unwrap()
        .component_with_mut(|c| c.uid() == "event-b")
        .unwrap()
        .set_summary(Some("Saved B".into()));

    store.save().unwrap();

    // Reload both dirs and check mutations survived.
    let reloaded_a = CalDir::new_from_dir(
        make_id("dir-a"),
        tmp_a.path().to_path_buf(),
        "DirA".into(),
        &Tz::UTC,
    )
    .unwrap();
    let reloaded_b = CalDir::new_from_dir(
        make_id("dir-b"),
        tmp_b.path().to_path_buf(),
        "DirB".into(),
        &Tz::UTC,
    )
    .unwrap();

    let sum_a = reloaded_a
        .file_by_id("event-a")
        .and_then(|f| f.components().first())
        .and_then(|c| c.summary().cloned());
    assert_eq!(sum_a.as_deref(), Some("Saved A"));

    let sum_b = reloaded_b
        .file_by_id("event-b")
        .and_then(|f| f.components().first())
        .and_then(|c| c.summary().cloned());
    assert_eq!(sum_b.as_deref(), Some("Saved B"));
}

// ---------------------------------------------------------------------------
// switch_directory
// ---------------------------------------------------------------------------

#[test]
fn switch_directory_success() {
    let tmp_a = TempDir::new().unwrap();
    let tmp_b = TempDir::new().unwrap();
    let src_path = copy_fixture("event_a.ics", &tmp_a);

    let id_a = make_id("dir-a");
    let id_b = make_id("dir-b");

    let dir_a = CalDir::new_from_dir(
        id_a.clone(),
        tmp_a.path().to_path_buf(),
        "DirA".into(),
        &Tz::UTC,
    )
    .unwrap();
    let dir_b = CalDir::new_from_dir(
        id_b.clone(),
        tmp_b.path().to_path_buf(),
        "DirB".into(),
        &Tz::UTC,
    )
    .unwrap();

    let mut store = CalStore::default();
    store.add(dir_a);
    store.add(dir_b);

    store
        .switch_directory(src_path.clone(), &id_a, &id_b)
        .unwrap();

    // The file must no longer be in dir-a.
    assert!(store.directory(&id_a).unwrap().files().is_empty());
    // The file must be in dir-b and accessible by UID.
    assert!(
        store
            .directory(&id_b)
            .unwrap()
            .file_by_id("event-a")
            .is_some()
    );
    // The old path is gone; a new file exists inside tmp_b.
    assert!(!src_path.exists());
    assert!(tmp_b.path().join("event_a.ics").exists());
}

#[test]
fn switch_directory_old_dir_not_found() {
    let tmp_b = TempDir::new().unwrap();

    let id_missing = make_id("no-such-dir");
    let id_b = make_id("dir-b");

    let dir_b = CalDir::new_from_dir(
        id_b.clone(),
        tmp_b.path().to_path_buf(),
        "DirB".into(),
        &Tz::UTC,
    )
    .unwrap();

    let mut store = CalStore::default();
    store.add(dir_b);

    let result = store.switch_directory(PathBuf::from("/irrelevant.ics"), &id_missing, &id_b);

    assert!(matches!(result, Err(ColError::DirNotFound(_))));
}

#[test]
fn switch_directory_new_dir_not_found_restores_file() {
    // The file is in old dir, but the target id does not exist. The method must return an error
    // and the file must still be accessible in the old directory.
    let tmp_a = TempDir::new().unwrap();
    let src_path = copy_fixture("event_a.ics", &tmp_a);

    let id_a = make_id("dir-a");
    let id_missing = make_id("no-such-dir");

    let dir_a = CalDir::new_from_dir(
        id_a.clone(),
        tmp_a.path().to_path_buf(),
        "DirA".into(),
        &Tz::UTC,
    )
    .unwrap();

    let mut store = CalStore::default();
    store.add(dir_a);

    let result = store.switch_directory(src_path.clone(), &id_a, &id_missing);

    assert!(matches!(result, Err(ColError::DirNotFound(_))));
    // The file must still be present in the old dir.
    assert!(
        store
            .directory(&id_a)
            .unwrap()
            .file_by_id("event-a")
            .is_some()
    );
}

#[test]
fn switch_directory_save_failure_rolls_back() {
    // Trigger the rollback branch: move the file to a new dir whose path points at a
    // non-existent directory on disk, so `file.save()` fails inside `switch_directory`.
    // After the failure the file must be restored to the old directory.
    let tmp_a = TempDir::new().unwrap();
    let src_path = copy_fixture("event_a.ics", &tmp_a);

    let id_a = make_id("dir-a");
    let id_b = make_id("dir-b");

    // dir-b points at a path that does not exist, so any save attempt will fail.
    let nonexistent_path = tmp_a.path().join("nonexistent_subdir");
    let dir_a = CalDir::new_from_dir(
        id_a.clone(),
        tmp_a.path().to_path_buf(),
        "DirA".into(),
        &Tz::UTC,
    )
    .unwrap();
    let dir_b = CalDir::new_empty(id_b.clone(), nonexistent_path, "DirB".into());

    let mut store = CalStore::default();
    store.add(dir_a);
    store.add(dir_b);

    let result = store.switch_directory(src_path, &id_a, &id_b);

    // save must have failed.
    assert!(result.is_err());
    // The file must have been rolled back to dir-a.
    assert!(
        store
            .directory(&id_a)
            .unwrap()
            .file_by_id("event-a")
            .is_some()
    );
    // dir-b must be empty.
    assert!(store.directory(&id_b).unwrap().files().is_empty());
}

// ---------------------------------------------------------------------------
// PartialEq / Debug
// ---------------------------------------------------------------------------

#[test]
fn partial_eq_and_debug() {
    let store_a = CalStore::default();
    let store_b = CalStore::default();
    assert_eq!(store_a, store_b);

    // Debug must not panic.
    let _ = format!("{store_a:?}");
}

// ---------------------------------------------------------------------------
// file_by_id across multiple dirs – UID in second dir
// ---------------------------------------------------------------------------

#[test]
fn file_by_id_found_in_second_dir() {
    let tmp_a = TempDir::new().unwrap();
    let tmp_b = TempDir::new().unwrap();
    let store = two_dir_store(&tmp_a, &["event_a.ics"], &tmp_b, &["event_b.ics"]);

    // event-b lives in dir-b (the second dir); make sure the store finds it.
    assert!(store.file_by_id("event-b").is_some());
    assert!(store.file_by_id("uid-absent").is_none());
}
