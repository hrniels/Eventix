// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{body::Body, http::Request};
use chrono::NaiveDate;
use eventix_ical::objects::{CalDate, EventLike};
use tempfile::TempDir;
use tokio::time::{Duration, sleep};
use tower::ServiceExt;

use crate::helper::edit::read_ics_by_uid;
use crate::helper::{CAL_ID, encode_form, make_router, make_state, post_query};

use super::{write_recurring_allday_event_ics, write_recurring_event_ics};

// --- POST /api/items/toggle ---

/// Toggling a recurring occurrence excludes it (adds an EXDATE to the base component).
#[tokio::test]
async fn toggle_excludes_occurrence() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "toggle-exclude";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // The first occurrence is 2026-04-15 (Wednesday 09:00 Europe/Berlin).
    let qs = encode_form(&[("uid", uid), ("rid", "TTEurope/Berlin;2026-04-15T09:00:00")]);
    let (status, _) = post_query(router, &format!("/api/items/toggle?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let base = ics
        .components()
        .iter()
        .find(|c| c.rid().is_none())
        .expect("expected base component");
    let exdates = base.as_event().unwrap().exdates();
    assert!(
        !exdates.is_empty(),
        "expected at least one EXDATE after toggle"
    );
    assert!(matches!(exdates[0], CalDate::DateTime(_)));
}

/// Toggling a recurring all-day occurrence stores EXDATE as VALUE=DATE.
#[tokio::test]
async fn toggle_excludes_all_day_occurrence_with_date_exdate() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "toggle-exclude-allday";
    write_recurring_allday_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);
    let router = make_router(state);

    // Toggle the first all-day occurrence, but send a timed RID as the UI/API currently does.
    let qs = encode_form(&[("uid", uid), ("rid", "TU2026-04-15T12:00:00")]);
    let (status, _) = post_query(router, &format!("/api/items/toggle?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let base = ics
        .components()
        .iter()
        .find(|c| c.rid().is_none())
        .expect("expected base component");
    let exdates = base.as_event().unwrap().exdates();
    assert_eq!(exdates.len(), 1);
    assert_eq!(
        exdates[0],
        CalDate::Date(
            NaiveDate::from_ymd_opt(2026, 4, 15).unwrap(),
            base.ctype().into(),
        )
    );
}

/// Toggling the same occurrence a second time re-includes it (removes the EXDATE).
#[tokio::test]
async fn toggle_twice_re_includes() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "toggle-twice";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);

    // First toggle: exclude.
    let router1 = make_router(state.clone());
    let qs = encode_form(&[("uid", uid), ("rid", "TTEurope/Berlin;2026-04-15T09:00:00")]);
    let (status, _) = post_query(router1, &format!("/api/items/toggle?{qs}")).await;
    assert_eq!(status, 200);

    // Second toggle: re-include.
    let router2 = make_router(state);
    let qs = encode_form(&[("uid", uid), ("rid", "TTEurope/Berlin;2026-04-15T09:00:00")]);
    let (status, _) = post_query(router2, &format!("/api/items/toggle?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let base = ics
        .components()
        .iter()
        .find(|c| c.rid().is_none())
        .expect("expected base component");
    let exdates = base.as_event().unwrap().exdates();
    assert!(
        exdates.is_empty(),
        "expected no EXDATEs after double toggle, found: {exdates:?}"
    );
}

/// Toggling the same all-day occurrence twice removes the DATE EXDATE again.
#[tokio::test]
async fn toggle_all_day_twice_re_includes() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "toggle-twice-allday";
    write_recurring_allday_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);

    let router1 = make_router(state.clone());
    let qs = encode_form(&[("uid", uid), ("rid", "TU2026-04-15T12:00:00")]);
    let (status, _) = post_query(router1, &format!("/api/items/toggle?{qs}")).await;
    assert_eq!(status, 200);

    let router2 = make_router(state);
    let qs = encode_form(&[("uid", uid), ("rid", "TU2026-04-15T12:00:00")]);
    let (status, _) = post_query(router2, &format!("/api/items/toggle?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let base = ics
        .components()
        .iter()
        .find(|c| c.rid().is_none())
        .expect("expected base component");
    assert!(base.as_event().unwrap().exdates().is_empty());
}

/// Attempting to toggle a non-existent UID returns an error.
#[tokio::test]
async fn unknown_uid_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_recurring_event_ics(&cal_dir, "other");
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[
        ("uid", "no-such-uid"),
        ("rid", "TTEurope/Berlin;2026-04-15T09:00:00"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/toggle?{qs}")).await;
    assert_ne!(status.as_u16(), 200);
}

#[tokio::test]
async fn toggle_continues_after_request_cancellation() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "toggle-cancelled";
    write_recurring_event_ics(&cal_dir, uid);
    let state = make_state(&cal_dir);

    let state_guard = state.lock().await;
    let router = make_router(state.clone());
    let qs = encode_form(&[("uid", uid), ("rid", "TTEurope/Berlin;2026-04-15T09:00:00")]);
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/items/toggle?{qs}"))
        .body(Body::empty())
        .unwrap();

    let handle = tokio::spawn(async move { router.oneshot(req).await });

    sleep(Duration::from_millis(20)).await;
    handle.abort();
    drop(state_guard);

    for _ in 0..20 {
        let ics = read_ics_by_uid(&cal_dir, uid);
        let base = ics
            .components()
            .iter()
            .find(|c| c.rid().is_none())
            .expect("expected base component");
        if !base.as_event().unwrap().exdates().is_empty() {
            return;
        }
        sleep(Duration::from_millis(20)).await;
    }

    panic!("item toggle did not finish after request cancellation");
}
