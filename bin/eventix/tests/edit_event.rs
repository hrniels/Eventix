// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod helper;

use chrono::NaiveDateTime;
use eventix_ical::objects::{CalDate, CalDateTime, CalRelated, CalTrigger, EventLike};
use tempfile::TempDir;

use helper::edit::{assert_success, mtime_nanos, read_ics_by_uid};
use helper::{
    CAL_ID, CAL2_ID, assert_error, assert_no_ics, encode_form, first_component, make_router,
    make_state, make_state_two_cals, merge_fields, post,
};

// --- Helpers specific to edit-event tests ---

/// Writes a minimal VEVENT ICS file for `uid` into `cal_dir` and returns the path.
///
/// The event has a fixed start/end on 2026-04-15 09:00–10:00 UTC with the given summary.
fn write_event_ics(cal_dir: &std::path::Path, uid: &str, summary: &str) -> std::path::PathBuf {
    let path = cal_dir.join(format!("{uid}.ics"));
    std::fs::write(
        &path,
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             DTSTART;TZID=Europe/Berlin:20260415T090000\r\n\
             DTEND;TZID=Europe/Berlin:20260415T100000\r\n\
             SUMMARY:{summary}\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
    path
}

/// Writes a minimal recurring weekly VEVENT ICS file for `uid` into `cal_dir`.
fn write_recurring_event_ics(cal_dir: &std::path::Path, uid: &str) -> std::path::PathBuf {
    let path = cal_dir.join(format!("{uid}.ics"));
    std::fs::write(
        &path,
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             DTSTART;TZID=Europe/Berlin:20260415T090000\r\n\
             DTEND;TZID=Europe/Berlin:20260415T100000\r\n\
             RRULE:FREQ=WEEKLY;BYDAY=WE\r\n\
             SUMMARY:Weekly standup\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
    path
}

/// Returns the base form fields every edit-event POST must include.
fn base_edit_fields(edit_start: &str) -> Vec<(&str, &str)> {
    vec![
        ("edit_start", edit_start),
        ("calendar", CAL_ID),
        ("location", ""),
        ("description", ""),
        ("start_end[from][date]", ""),
        ("start_end[from][time]", ""),
        ("start_end[to][date]", ""),
        ("start_end[to][time]", ""),
        ("start_end[timezone]", "Europe/Berlin"),
        ("rrule[freq]", "NONE"),
        ("rrule[interval]", "1"),
        ("rrule[end]", "NoEnd"),
        ("rrule[count]", "1"),
        ("rrule[weekly_days]", ""),
        ("rrule[monthly_type]", "None"),
        ("rrule[yearly_type]", "None"),
        ("alarm[calendar][trigger]", "NONE"),
        ("alarm[calendar][duration]", "30"),
        ("alarm[calendar][durunit]", "Minutes"),
        ("alarm[calendar][durtype]", "BeforeStart"),
    ]
}

// --- Series edits ---

/// Editing the summary, location, and description of a simple event (Series mode).
#[tokio::test]
async fn series_edit_basic_fields() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-basic";
    let ics_path = write_event_ics(&cal_dir, uid, "Original title");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "Updated title"),
            ("location", "Room 42"),
            ("description", "A new description"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = first_component(&ics);
    assert_eq!(comp.summary(), Some(&"Updated title".to_string()));
    assert_eq!(comp.location(), Some(&"Room 42".to_string()));
    assert_eq!(comp.description(), Some(&"A new description".to_string()));
}

/// Adding a relative alarm to an event that previously had none (Series mode).
#[tokio::test]
async fn series_edit_add_relative_alarm() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-alarm";
    let ics_path = write_event_ics(&cal_dir, uid, "Doctor");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "Doctor"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][trigger]", "RELATIVE"),
            ("alarm[calendar][duration]", "15"),
            ("alarm[calendar][durunit]", "Minutes"),
            ("alarm[calendar][durtype]", "BeforeStart"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = first_component(&ics);
    let alarms = comp.alarms().expect("expected VALARM");
    assert_eq!(alarms.len(), 1);

    match alarms[0].trigger() {
        CalTrigger::Relative { related, duration } => {
            assert_eq!(related, &CalRelated::Start);
            assert_eq!(duration.num_minutes(), -15);
        }
        other => panic!("expected relative trigger, got {:?}", other),
    }
}

/// Editing an event to add a recurrence rule (Series mode). Verifies RRULE in the output.
#[tokio::test]
async fn series_edit_add_rrule() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-rrule";
    let ics_path = write_event_ics(&cal_dir, uid, "Weekly meeting");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "Weekly meeting"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "WEEKLY"),
            ("rrule[weekly_days]", "MO,"),
            ("rrule[end]", "Count"),
            ("rrule[count]", "4"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = first_component(&ics);
    let rrule = comp.rrule().expect("expected RRULE");
    assert_eq!(rrule.count(), Some(4));
}

// --- Cross-calendar edit ---

/// Editing an event and changing its calendar (Series mode). Verifies that the ICS file is moved
/// to the target calendar directory and no longer exists in the source directory.
#[tokio::test]
async fn series_edit_moves_to_different_calendar() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-move-cal";
    let ics_path = write_event_ics(&cal_dir, uid, "Move me");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let (state, cal2_dir) = make_state_two_cals(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("calendar", CAL2_ID),
            ("summary", "Move me"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    // The file must no longer exist in the source calendar.
    assert!(
        !ics_path.exists(),
        "ICS file must be removed from source calendar after move"
    );

    // The file must now exist in the target calendar directory.
    let new_ics_path = cal2_dir.join(format!("{uid}.ics"));
    assert!(
        new_ics_path.exists(),
        "ICS file must be present in target calendar after move"
    );

    // The moved file must retain the event's content.
    let ics = read_ics_by_uid(&cal2_dir, uid);
    let comp = first_component(&ics);
    assert_eq!(comp.summary(), Some(&"Move me".to_string()));
}

// --- Occurrence edit ---

/// Editing a single occurrence of a recurring event (Occurrence mode). Verifies that a
/// RECURRENCE-ID overwrite component is added to the file.
#[tokio::test]
async fn occurrence_edit_overrides_single() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-occ";
    let ics_path = write_recurring_event_ics(&cal_dir, uid);
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Edit the occurrence on 2026-04-15 (the first Wednesday)
    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "Special standup"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "11:00"), // extended end
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);
    // Occurrence mode: no calendar field, rid = the occurrence date in CalDate display format.
    // CalDate::DateTime(CalDateTime::Timezone("Europe/Berlin", 2026-04-15T09:00:00)) serialises
    // as "TTEurope/Berlin;2026-04-15T09:00:00" (T prefix for CalDate, then T prefix for
    // CalDateTime::Timezone, then tz;datetime).
    let uri = format!(
        "/pages/items/edit?mode=Occurrence&uid={uid}&rid=TTEurope%2FBerlin%3B2026-04-15T09%3A00%3A00&prev=%2F"
    );

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_ics_by_uid(&cal_dir, uid);
    // There should now be two components: the base and the overwrite
    let comps = ics.components();
    assert_eq!(comps.len(), 2, "expected base + one overwrite component");
    let overwrite = comps
        .iter()
        .find(|c: &&eventix_ical::objects::CalComponent| c.rid().is_some())
        .expect("expected overwrite");
    assert_eq!(overwrite.summary(), Some(&"Special standup".to_string()));
}

// --- Following edit ---

/// Splitting a recurring series at an occurrence (Following mode). Verifies that:
/// - the original series retains its old properties (summary, start, end) and gains an UNTIL,
/// - the new series carries the properties specified in the edit request.
#[tokio::test]
async fn following_edit_splits_series() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-following";
    let ics_path = write_recurring_event_ics(&cal_dir, uid);
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "New series from here"),
            ("start_end[from][date]", "2026-04-22"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-22"),
            ("start_end[to][time]", "11:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "WEEKLY"),
            ("rrule[weekly_days]", "WE,"),
            ("rrule[end]", "NoEnd"),
        ],
    );
    let body = encode_form(&fields);
    // Following mode: rid = the occurrence date in CalDate display format.
    let uri = format!(
        "/pages/items/edit?mode=Following&uid={uid}&rid=TTEurope%2FBerlin%3B2026-04-22T09%3A00%3A00&prev=%2F"
    );

    let (status, resp_body): (_, String) = post(router, &uri, &body).await;
    assert_eq!(status, 200);

    // On Following success the handler renders the edit form for the new series (no error banner).
    assert!(
        !resp_body.contains("ev_msg_error"),
        "unexpected error: {resp_body}"
    );

    // --- Original series ---
    // Must retain its old summary, start (09:00), and end (10:00), and gain an UNTIL.
    let original = read_ics_by_uid(&cal_dir, uid);
    let orig_comp = first_component(&original);
    assert_eq!(
        orig_comp.summary(),
        Some(&"Weekly standup".to_string()),
        "original series must keep its old summary"
    );
    let orig_start = match orig_comp.start().expect("expected DTSTART on original") {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => *dt,
        other => panic!("expected Timezone DTSTART on original, got {:?}", other),
    };
    assert_eq!(
        orig_start,
        NaiveDateTime::parse_from_str("2026-04-15 09:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        "original series must keep its old start time"
    );
    let orig_end = match orig_comp.end_or_due().expect("expected DTEND on original") {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => *dt,
        other => panic!("expected Timezone DTEND on original, got {:?}", other),
    };
    assert_eq!(
        orig_end,
        NaiveDateTime::parse_from_str("2026-04-15 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        "original series must keep its old end time"
    );
    let orig_rrule = orig_comp
        .rrule()
        .expect("expected RRULE on original series");
    assert!(
        orig_rrule.until().is_some(),
        "original series must gain an UNTIL"
    );

    // --- New series ---
    // A second ICS file must exist; find it by excluding the original uid.
    let new_ics_path = std::fs::read_dir(&cal_dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("ics")
                && p.file_stem().and_then(|s| s.to_str()) != Some(uid)
            {
                Some(p)
            } else {
                None
            }
        })
        .next()
        .expect("expected a second ICS file for the new series");
    let new_tz = chrono_tz::UTC;
    let new_cal = eventix_ical::col::CalFile::new_from_file(
        std::sync::Arc::new(CAL_ID.to_string()),
        new_ics_path,
        &new_tz,
    )
    .unwrap();
    let new_comp = first_component(&new_cal);
    assert_eq!(
        new_comp.summary(),
        Some(&"New series from here".to_string()),
        "new series must have the summary from the edit request"
    );
    let new_start = match new_comp.start().expect("expected DTSTART on new series") {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => *dt,
        other => panic!("expected Timezone DTSTART on new series, got {:?}", other),
    };
    assert_eq!(
        new_start,
        NaiveDateTime::parse_from_str("2026-04-22 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        "new series must have the start time from the edit request"
    );
    let new_end = match new_comp.end_or_due().expect("expected DTEND on new series") {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => *dt,
        other => panic!("expected Timezone DTEND on new series, got {:?}", other),
    };
    assert_eq!(
        new_end,
        NaiveDateTime::parse_from_str("2026-04-22 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        "new series must have the end time from the edit request"
    );
    let new_rrule = new_comp.rrule().expect("expected RRULE on new series");
    assert!(
        new_rrule.until().is_none(),
        "new series must not have an UNTIL"
    );
}

/// Splitting at the very first occurrence (Following mode) leaves the original series with no
/// occurrences, so its ICS file must be deleted. Only the new series file must remain.
#[tokio::test]
async fn following_edit_empty_original_deletes_file() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-following-empty";
    let ics_path = write_recurring_event_ics(&cal_dir, uid);
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Split at the very first occurrence (2026-04-15 09:00), so the original series has no
    // occurrences before the split point and its file must be deleted.
    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "Renamed from first"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "WEEKLY"),
            ("rrule[weekly_days]", "WE,"),
            ("rrule[end]", "NoEnd"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!(
        "/pages/items/edit?mode=Following&uid={uid}&rid=TTEurope%2FBerlin%3B2026-04-15T09%3A00%3A00&prev=%2F"
    );

    let (status, resp_body): (_, String) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert!(
        !resp_body.contains("ev_msg_error"),
        "unexpected error: {resp_body}"
    );

    // The original file must have been deleted.
    assert!(
        !ics_path.exists(),
        "original ICS file must be deleted when the original series is empty"
    );

    // Exactly one ICS file must remain: the new series.
    let remaining: Vec<_> = std::fs::read_dir(&cal_dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("ics") {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        remaining.len(),
        1,
        "expected exactly 1 ICS file after empty-original split"
    );

    // The remaining file is the new series and must carry the properties from the edit request.
    let new_cal = eventix_ical::col::CalFile::new_from_file(
        std::sync::Arc::new(CAL_ID.to_string()),
        remaining[0].clone(),
        &chrono_tz::UTC,
    )
    .unwrap();
    let new_comp = first_component(&new_cal);
    assert_eq!(
        new_comp.summary(),
        Some(&"Renamed from first".to_string()),
        "new series must have the summary from the edit request"
    );
    let new_start = match new_comp.start().expect("expected DTSTART on new series") {
        CalDate::DateTime(CalDateTime::Timezone(dt, _)) => *dt,
        other => panic!("expected Timezone DTSTART on new series, got {:?}", other),
    };
    assert_eq!(
        new_start,
        NaiveDateTime::parse_from_str("2026-04-15 09:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        "new series must have the start time from the edit request"
    );
}

// --- Error paths ---

/// An edit with an edit_start older than the file's mtime is rejected with a staleness error.
#[tokio::test]
async fn series_edit_stale_rejected() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-stale";
    write_event_ics(&cal_dir, uid, "Stale event");

    let state = make_state(&cal_dir);
    let router = make_router(state);

    // edit_start = 0 is always less than the real mtime → staleness check fires
    let fields = merge_fields(
        base_edit_fields("0"),
        &[
            ("summary", "Should not save"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
}

/// An edit referencing a UID that does not exist returns an error (500 / HTMLError).
#[tokio::test]
async fn series_edit_unknown_uid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields("0"),
        &[
            ("summary", "Ghost event"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);
    let uri = "/pages/items/edit?mode=Series&uid=does-not-exist&prev=%2F";

    let (status, _resp_body) = post(router, uri, &body).await;
    // The handler returns an HTMLError (anyhow error) which axum converts to a 500.
    assert_eq!(status, 500);
    assert_no_ics(&cal_dir);
}

/// An edit with an empty summary is rejected.
#[tokio::test]
async fn series_edit_missing_summary() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-no-summary";
    let ics_path = write_event_ics(&cal_dir, uid, "Has a summary");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", ""),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
}

/// An edit with missing start datetime (from_enabled absent) is rejected for events.
#[tokio::test]
async fn series_edit_missing_start() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-no-start";
    let ics_path = write_event_ics(&cal_dir, uid, "No start");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    // from_enabled absent → start is None → error.start_datetime
    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "No start event"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[to_enabled]", "true"),
            // from_enabled intentionally absent
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
}

/// An edit with end before start is rejected.
#[tokio::test]
async fn series_edit_end_before_start() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-end-before-start";
    let ics_path = write_event_ics(&cal_dir, uid, "Backwards");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "Backwards event"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "09:00"), // end < start
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
}

/// An edit with a start in the Europe/Berlin spring-forward DST gap is rejected.
#[tokio::test]
async fn series_edit_start_in_dst_gap() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-dst-gap";
    let ics_path = write_event_ics(&cal_dir, uid, "DST gap");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "DST gap event"),
            // 2026-03-29 02:30 does not exist in Europe/Berlin
            ("start_end[from][date]", "2026-03-29"),
            ("start_end[from][time]", "02:30"),
            ("start_end[to][date]", "2026-03-29"),
            ("start_end[to][time]", "03:30"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
}

/// An edit with a start in the Europe/Berlin autumn DST fold (ambiguous) is rejected.
#[tokio::test]
async fn series_edit_start_in_dst_fold() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-dst-fold";
    let ics_path = write_event_ics(&cal_dir, uid, "DST fold");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "DST fold event"),
            // 2026-10-25 02:30 is ambiguous in Europe/Berlin
            ("start_end[from][date]", "2026-10-25"),
            ("start_end[from][time]", "02:30"),
            ("start_end[to][date]", "2026-10-25"),
            ("start_end[to][time]", "03:30"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
}

/// An absolute alarm with no datetime is rejected during a Series edit.
#[tokio::test]
async fn series_edit_absolute_alarm_missing_datetime() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();

    let uid = "edit-event-abs-alarm-missing";
    let ics_path = write_event_ics(&cal_dir, uid, "Alarm test");
    let edit_start = mtime_nanos(&ics_path).to_string();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_edit_fields(&edit_start),
        &[
            ("summary", "Alarm test"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][trigger]", "ABSOLUTE"),
            // datetime fields intentionally absent
        ],
    );
    let body = encode_form(&fields);
    let uri = format!("/pages/items/edit?mode=Series&uid={uid}&prev=%2F");

    let (status, resp_body) = post(router, &uri, &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
}
