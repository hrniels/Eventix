// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use tempfile::TempDir;

use crate::helper::{
    CAL_ID, COL_ID, encode_form, get, make_router, make_state, make_state_with_email,
};

use super::write_recurring_event_ics;

// --- GET /api/items/details ---

/// Writes a minimal timed VEVENT ICS for `uid` into `cal_dir`.
fn write_details_event_ics(cal_dir: &Path, uid: &str, summary: &str) {
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
}

/// Writes a VEVENT ICS carrying an ORGANIZER and an ATTENDEE for `uid` into `cal_dir`.
fn write_event_with_organizer_ics(cal_dir: &Path, uid: &str) {
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
             SUMMARY:Team sync\r\n\
             ORGANIZER;CN=Organizer:mailto:organizer@example.com\r\n\
             ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:test@example.com\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
}

/// Writes a weekly recurring VEVENT ICS with ORGANIZER + ATTENDEE for `uid` into `cal_dir`.
fn write_recurring_event_with_organizer_ics(cal_dir: &Path, uid: &str) {
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
             SUMMARY:Recurring sync\r\n\
             ORGANIZER;CN=Organizer:mailto:organizer@example.com\r\n\
             ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:test@example.com\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
}

/// Fetching the details for an existing event returns HTTP 200 and a JSON body whose `html` key
/// contains the rendered detail snippet.
#[tokio::test]
async fn details_returns_html() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "details-event";
    write_details_event_ics(&cal_dir, uid, "Important meeting");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = format!("/api/items/details?uid={uid}&edit=false");
    let (status, body) = get(router, &uri).await;
    assert_eq!(status, 200);

    // The response is a JSON object with an "html" key.
    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("response is not valid JSON: {body}"));
    assert!(
        json.get("html").and_then(|v| v.as_str()).is_some(),
        "expected 'html' key in response JSON, got: {json}"
    );
}

/// Fetching the details for a recurring event returns HTTP 200. This exercises the
/// `occ.is_recurrent()` branch and the per-occurrence attendee-status lookup.
#[tokio::test]
async fn details_recurring_event_returns_html() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "details-recurring";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Request a specific occurrence by rid.
    let qs = encode_form(&[
        ("uid", uid),
        ("rid", "TTEurope/Berlin;2026-04-15T09:00:00"),
        ("edit", "false"),
    ]);
    let (status, body) = get(router, &format!("/api/items/details?{qs}")).await;
    assert_eq!(status, 200);
    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("response is not valid JSON: {body}"));
    assert!(json.get("html").and_then(|v| v.as_str()).is_some());
}

/// Fetching the details with `edit=true` and a `rid` exercises the `edit_modes` branch (owner +
/// rid present).
#[tokio::test]
async fn details_edit_mode_with_rid_returns_html() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "details-edit-rid";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[
        ("uid", uid),
        ("rid", "TTEurope/Berlin;2026-04-15T09:00:00"),
        ("edit", "true"),
    ]);
    let (status, body) = get(router, &format!("/api/items/details?{qs}")).await;
    assert_eq!(status, 200);
    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("response is not valid JSON: {body}"));
    assert!(json.get("html").and_then(|v| v.as_str()).is_some());
}

/// Fetching the details for an event with an organizer exercises the organizer template branch.
#[tokio::test]
async fn details_event_with_organizer_returns_html() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp
        .path()
        .join("vdirsyncer")
        .join(format!("{COL_ID}-data"))
        .join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "details-organizer";
    write_event_with_organizer_ics(&cal_dir, uid);
    let (state, _) = make_state_with_email(&tmp);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("edit", "false")]);
    let (status, body) = get(router, &format!("/api/items/details?{qs}")).await;
    assert_eq!(status, 200);
    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("response is not valid JSON: {body}"));
    assert!(json.get("html").and_then(|v| v.as_str()).is_some());
}

/// Fetching the details for a recurring event that has an organizer and an attendee exercises the
/// `series_partstat` and per-occurrence attendee-status branches.
#[tokio::test]
async fn details_recurring_with_organizer_and_attendee_returns_html() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp
        .path()
        .join("vdirsyncer")
        .join(format!("{COL_ID}-data"))
        .join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "details-recurring-org";
    write_recurring_event_with_organizer_ics(&cal_dir, uid);
    let (state, _) = make_state_with_email(&tmp);
    let router = make_router(state);

    let qs = encode_form(&[
        ("uid", uid),
        ("rid", "TTEurope/Berlin;2026-04-15T09:00:00"),
        ("edit", "false"),
    ]);
    let (status, body) = get(router, &format!("/api/items/details?{qs}")).await;
    assert_eq!(status, 200);
    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("response is not valid JSON: {body}"));
    assert!(json.get("html").and_then(|v| v.as_str()).is_some());
}

/// Fetching the details for an unknown UID returns a non-200 status.
#[tokio::test]
async fn details_unknown_uid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    // At least one ICS must be present so the router initialises state.
    write_details_event_ics(&cal_dir, "other-event", "Other");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let (status, _) = get(router, "/api/items/details?uid=no-such-uid&edit=false").await;
    assert_ne!(status.as_u16(), 200);
}
