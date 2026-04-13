// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_ical::objects::EventLike;
use tempfile::TempDir;

use crate::helper::edit::read_ics_by_uid;
use crate::helper::{CAL_ID, encode_form, make_router, make_state, post_query};

use super::write_recurring_event_ics;

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
