// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use eventix_ical::objects::{CalPartStat, EventLike};
use tempfile::TempDir;

use crate::helper::edit::read_ics_by_uid;
use crate::helper::{CAL_ID, COL_ID, encode_form, make_router, make_state_with_email, post_query};

use super::{write_event_ics, write_recurring_event_ics};

// --- POST /api/items/respond ---

/// Returns the calendar directory path for a VDirSyncer-backed state created with `tmp`.
///
/// The directory is pre-created so that ICS fixtures can be written into it before
/// `make_state_with_email` is called.  The directory must be populated before the state is
/// constructed because `State::new` scans for existing ICS files at startup.
fn make_email_cal_dir(tmp: &TempDir) -> PathBuf {
    let cal_dir = tmp
        .path()
        .join("vdirsyncer")
        .join(format!("{COL_ID}-data"))
        .join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    cal_dir
}

/// Accepting an invitation on a simple event stores PARTSTAT:ACCEPTED on the attendee entry.
#[tokio::test]
async fn respond_accept_sets_partstat() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = make_email_cal_dir(&tmp);
    let uid = "respond-accept";
    write_event_ics(&cal_dir, uid, "Team meeting");
    let (state, _) = make_state_with_email(&tmp);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("stat", "Accept")]);
    let (status, _) = post_query(router, &format!("/api/items/respond?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let atts = comp.attendees().expect("expected attendees");
    let att = atts
        .iter()
        .find(|a| a.address().to_lowercase() == "test@example.com")
        .expect("attendee not found");
    assert_eq!(att.part_stat(), Some(CalPartStat::Accepted));
}

/// Declining an invitation sets PARTSTAT:DECLINED.
#[tokio::test]
async fn respond_decline_sets_partstat() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = make_email_cal_dir(&tmp);
    let uid = "respond-decline";
    write_event_ics(&cal_dir, uid, "Unwanted meeting");
    let (state, _) = make_state_with_email(&tmp);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("stat", "Decline")]);
    let (status, _) = post_query(router, &format!("/api/items/respond?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comp = ics.components().first().unwrap();
    let atts = comp.attendees().expect("expected attendees");
    let att = atts
        .iter()
        .find(|a| a.address().to_lowercase() == "test@example.com")
        .expect("attendee not found");
    assert_eq!(att.part_stat(), Some(CalPartStat::Declined));
}

/// Tentatively accepting a recurring occurrence creates a RECURRENCE-ID override.
#[tokio::test]
async fn respond_tentative_recurring_creates_override() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = make_email_cal_dir(&tmp);
    let uid = "respond-tentative-recurring";
    write_recurring_event_ics(&cal_dir, uid);
    let (state, _) = make_state_with_email(&tmp);
    let router = make_router(state);

    let qs = encode_form(&[
        ("uid", uid),
        ("rid", "TTEurope/Berlin;2026-04-15T09:00:00"),
        ("stat", "Tentative"),
    ]);
    let (status, _) = post_query(router, &format!("/api/items/respond?{qs}")).await;
    assert_eq!(status, 200);

    let ics = read_ics_by_uid(&cal_dir, uid);
    let comps = ics.components();
    let override_comp = comps
        .iter()
        .find(|c| c.rid().is_some())
        .expect("expected a RECURRENCE-ID override");
    let atts = override_comp.attendees().expect("expected attendees");
    let att = atts
        .iter()
        .find(|a| a.address().to_lowercase() == "test@example.com")
        .expect("attendee not found");
    assert_eq!(att.part_stat(), Some(CalPartStat::Tentative));
}

/// Supplying an invalid `stat` string returns a non-200 status (deserialization failure).
#[tokio::test]
async fn invalid_stat_returns_error() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = make_email_cal_dir(&tmp);
    let uid = "respond-invalid";
    write_event_ics(&cal_dir, uid, "Meeting");
    let (state, _) = make_state_with_email(&tmp);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("stat", "Maybe")]);
    let (status, _) = post_query(router, &format!("/api/items/respond?{qs}")).await;
    assert_ne!(status.as_u16(), 200);
}

/// When no email account is configured on the collection, the endpoint returns an error.
#[tokio::test]
async fn no_email_account_returns_error() {
    use crate::helper::make_state;
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let uid = "respond-noemail";
    write_event_ics(&cal_dir, uid, "Meeting");
    // FileSystem collection has no email account.
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let qs = encode_form(&[("uid", uid), ("stat", "Accept")]);
    let (status, _) = post_query(router, &format!("/api/items/respond?{qs}")).await;
    assert_eq!(status.as_u16(), 100);
}
