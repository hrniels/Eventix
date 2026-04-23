// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../helper/mod.rs"]
mod helper;

use std::path::Path;

use axum::http::StatusCode;
use eventix_locale::LocaleType;
use eventix_state::{CalendarSettings, CollectionSettings, SyncerType};
use serde_json::from_str;
use tempfile::TempDir;

use helper::{CAL_ID, get, make_calendars_api_router, make_state, make_state_from_col, post_query};

fn write_event_with_attendees(cal_dir: &Path, uid: &str, attendees: &[&str]) {
    let path = cal_dir.join(format!("{uid}.ics"));
    let attendees = attendees
        .iter()
        .map(|attendee| format!("{attendee}\r\n"))
        .collect::<String>();

    std::fs::write(
        path,
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             DTSTART;TZID=Europe/Berlin:20260415T090000\r\n\
             DTEND;TZID=Europe/Berlin:20260415T100000\r\n\
             SUMMARY:Meeting\r\n\
             {attendees}\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
}

fn make_collection(tmp: &TempDir) -> CollectionSettings {
    let calendars_path = tmp.path().join("calendars");
    let cal_dir = calendars_path.join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).expect("create calendar dir");

    let mut collection = CollectionSettings::new(SyncerType::FileSystem {
        path: calendars_path.to_string_lossy().into_owned(),
    });
    let mut calendar = CalendarSettings::default();
    calendar.set_enabled(true);
    calendar.set_folder(CAL_ID.to_string());
    calendar.set_name("Test Calendar".to_string());
    collection
        .all_calendars_mut()
        .insert(CAL_ID.to_string(), calendar);
    collection
}

// --- GET /api/attendees ---

#[tokio::test]
async fn attendees_returns_sorted_formatted_matches() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_with_attendees(
        &cal_dir,
        "attendees-a",
        &[
            "ATTENDEE;CN=Aaron Example:mailto:aaron@example.com",
            "ATTENDEE:bob@example.com",
        ],
    );
    let state = make_state(&cal_dir);
    let router = make_calendars_api_router(state);

    let (status, body) = get(router, "/api/attendees?term=example.com").await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");
    assert_eq!(
        from_str::<Vec<String>>(&body).expect("parse attendees response"),
        vec![
            "Aaron Example <aaron@example.com>".to_string(),
            "bob@example.com".to_string(),
        ]
    );
}

#[tokio::test]
async fn attendees_returns_empty_list_for_no_matches() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    write_event_with_attendees(
        &cal_dir,
        "attendees-b",
        &["ATTENDEE;CN=Aaron Example:mailto:aaron@example.com"],
    );
    let state = make_state(&cal_dir);
    let router = make_calendars_api_router(state);

    let (status, body) = get(router, "/api/attendees?term=nomatch").await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");
    assert_eq!(
        from_str::<Vec<String>>(&body).expect("parse attendees response"),
        Vec::<String>::new()
    );
}

// --- POST /api/setlang ---

#[tokio::test]
async fn setlang_updates_locale_and_persists_misc_state() {
    let source_tmp = TempDir::new().unwrap();
    let (state, xdg_tmp) = make_state_from_col(make_collection(&source_tmp));
    std::fs::write(
        xdg_tmp.path().join("data/locale/German.toml"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/locale/German.toml"
        )),
    )
    .unwrap();

    {
        let locked = state.lock().await;
        assert_eq!(locked.misc().locale_type(), LocaleType::English);
        assert_eq!(locked.locale().ty(), LocaleType::English);
    }

    let router = make_calendars_api_router(state.clone());
    let (status, body) = post_query(router, "/api/setlang?lang=German").await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");
    assert_eq!(body, "null");

    let locked = state.lock().await;
    assert_eq!(locked.misc().locale_type(), LocaleType::German);
    assert_eq!(locked.locale().ty(), LocaleType::German);

    let misc = std::fs::read_to_string(xdg_tmp.path().join("data/misc.toml")).unwrap();
    assert!(misc.contains("locale_type = \"German\""));
}

#[tokio::test]
async fn setlang_rejects_invalid_locale() {
    let source_tmp = TempDir::new().unwrap();
    let (state, _xdg_tmp) = make_state_from_col(make_collection(&source_tmp));

    let router = make_calendars_api_router(state);
    let (status, _body) = post_query(router, "/api/setlang?lang=Spanish").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// --- POST /api/togglecal ---

#[tokio::test]
async fn togglecal_toggles_calendar_and_persists_misc_state() {
    let source_tmp = TempDir::new().unwrap();
    let (state, xdg_tmp) = make_state_from_col(make_collection(&source_tmp));

    let router1 = make_calendars_api_router(state.clone());
    let (status1, body1) = post_query(router1, &format!("/api/togglecal?id={CAL_ID}")).await;
    assert_eq!(status1, StatusCode::OK, "unexpected body:\n{body1}");
    assert_eq!(body1, "{}");

    {
        let locked = state.lock().await;
        assert!(locked.misc().calendar_disabled(&CAL_ID.to_string()));
    }
    let misc_after_disable =
        std::fs::read_to_string(xdg_tmp.path().join("data/misc.toml")).unwrap();
    assert!(misc_after_disable.contains(&format!("disabled_calendars = [\"{CAL_ID}\"]")));

    let router2 = make_calendars_api_router(state.clone());
    let (status2, body2) = post_query(router2, &format!("/api/togglecal?id={CAL_ID}")).await;
    assert_eq!(status2, StatusCode::OK, "unexpected body:\n{body2}");
    assert_eq!(body2, "{}");

    {
        let locked = state.lock().await;
        assert!(!locked.misc().calendar_disabled(&CAL_ID.to_string()));
    }
    let misc_after_enable = std::fs::read_to_string(xdg_tmp.path().join("data/misc.toml")).unwrap();
    assert!(!misc_after_enable.contains(CAL_ID));
}
