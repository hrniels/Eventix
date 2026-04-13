// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use tempfile::TempDir;

use crate::helper::{CAL_ID, get, make_router, make_state};

use super::write_recurring_event_ics;

// --- GET /api/items/occlist ---

/// Requesting occurrences in the Forward direction from a date before the series returns a list
/// of the first `count` occurrences and a JSON body with `html` and `date` fields.
#[tokio::test]
async fn occlist_forward_returns_html_and_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "occlist-fwd";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Request 2 occurrences forward from 2026-04-01.
    let uri =
        format!("/api/items/occlist?uid={uid}&date=D2026-04-01%3BInclusive&dir=Forward&count=2");
    let (status, body) = get(router, &uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("response is not valid JSON: {body}"));
    assert!(
        json.get("html").and_then(|v| v.as_str()).is_some(),
        "expected 'html' key, got: {json}"
    );
    // 'date' may be null if there are fewer than count+1 occurrences, but the key must be present.
    assert!(
        json.get("date").is_some(),
        "expected 'date' key, got: {json}"
    );
}

/// Requesting occurrences in the ForwardFrom direction starting at the first occurrence's date
/// returns that occurrence as the first element.
#[tokio::test]
async fn occlist_forward_from() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "occlist-ff";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri = format!(
        "/api/items/occlist?uid={uid}&date=D2026-04-15%3BInclusive&dir=ForwardFrom&count=1"
    );
    let (status, body) = get(router, &uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    let html = json["html"].as_str().unwrap();
    // The HTML should contain the occurrence's summary.
    assert!(
        html.contains("Weekly standup"),
        "expected occurrence in HTML, got: {html}"
    );
}

/// Requesting occurrences in the Backwards direction from a date after the first occurrence
/// returns earlier occurrences.
#[tokio::test]
async fn occlist_backwards() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "occlist-back";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let uri =
        format!("/api/items/occlist?uid={uid}&date=D2026-05-01%3BInclusive&dir=Backwards&count=2");
    let (status, body) = get(router, &uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json.get("html").is_some());
}

/// Requesting more occurrences than available returns a null `date` field (no next page).
#[tokio::test]
async fn occlist_no_more_pages_returns_null_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "occlist-nomore";

    // Write a recurring event with only 2 occurrences via COUNT.
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
             RRULE:FREQ=WEEKLY;COUNT=2\r\n\
             SUMMARY:Limited series\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();

    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Request more than the available 2 occurrences.
    let uri =
        format!("/api/items/occlist?uid={uid}&date=D2026-04-01%3BInclusive&dir=Forward&count=10");
    let (status, body) = get(router, &uri).await;
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        json["date"].is_null(),
        "expected null 'date' when no more pages, got: {}",
        json["date"]
    );
}

/// Supplying an unknown UID returns a non-200 status.
#[tokio::test]
async fn unknown_uid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_recurring_event_ics(&cal_dir, "some-uid");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let (status, _) = get(
        router,
        "/api/items/occlist?uid=no-such-uid&date=D2026-04-01%3BInclusive&dir=Forward&count=5",
    )
    .await;
    assert_ne!(status.as_u16(), 200);
}
