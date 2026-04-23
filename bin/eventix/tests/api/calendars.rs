// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../helper/mod.rs"]
mod helper;

use axum::http::StatusCode;
use eventix_ical::objects::CalCompType;
use eventix_state::{CalendarAlarmType, CollectionSettings, SyncerType};
use serde_json::Value;
use tempfile::TempDir;

use helper::{
    CAL_ID, COL_ID, encode_form, make_calendars_api_router, make_state_from_col, post, post_query,
};

fn make_filesystem_collection(tmp: &TempDir) -> CollectionSettings {
    let calendars_path = tmp.path().join("calendars");
    std::fs::create_dir_all(&calendars_path).expect("create calendars dir");

    CollectionSettings::new(SyncerType::FileSystem {
        path: calendars_path.to_string_lossy().into_owned(),
    })
}

fn make_collection_with_calendar(tmp: &TempDir) -> CollectionSettings {
    let mut col = make_filesystem_collection(tmp);
    let mut cal = eventix_state::CalendarSettings::default();
    cal.set_enabled(true);
    cal.set_folder(CAL_ID.to_string());
    cal.set_name("Test Calendar".to_string());
    col.all_calendars_mut().insert(CAL_ID.to_string(), cal);
    std::fs::create_dir_all(tmp.path().join("calendars").join(CAL_ID)).expect("create calendar");
    col
}

fn response_json(body: &str) -> Value {
    serde_json::from_str(body).expect("parse JSON response")
}

#[tokio::test]
async fn add_calendar_creates_calendar() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_filesystem_collection(&tmp));

    let router = make_calendars_api_router(state.clone());
    let (status, resp) = post_query(
        router,
        &format!("/api/calendars/addcal?col_id={COL_ID}&name=Local%20Team"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{resp}");

    let locked = state.lock().await;
    let calendars = locked
        .settings()
        .collections()
        .get(COL_ID)
        .unwrap()
        .all_calendars();
    let added = calendars
        .values()
        .next()
        .expect("expected inserted calendar");
    assert_eq!(added.name(), "Local Team");
    assert_eq!(added.folder(), "local-team");
    assert!(added.enabled());
    assert!(tmp.path().join("calendars/local-team").exists());
}

#[tokio::test]
async fn add_calendar_sanitizes_and_deduplicates_folder_name() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_filesystem_collection(&tmp));

    let router1 = make_calendars_api_router(state.clone());
    let (status1, resp1) = post_query(
        router1,
        &format!("/api/calendars/addcal?col_id={COL_ID}&name=Team%20%26%20Ops"),
    )
    .await;
    assert_eq!(status1, StatusCode::OK, "unexpected body:\n{resp1}");

    let router2 = make_calendars_api_router(state.clone());
    let (status2, resp2) = post_query(
        router2,
        &format!("/api/calendars/addcal?col_id={COL_ID}&name=Team%20Ops"),
    )
    .await;
    assert_eq!(status2, StatusCode::OK, "unexpected body:\n{resp2}");

    let locked = state.lock().await;
    let mut folders = locked
        .settings()
        .collections()
        .get(COL_ID)
        .unwrap()
        .all_calendars()
        .values()
        .map(|cal| cal.folder().clone())
        .collect::<Vec<_>>();
    folders.sort();

    assert_eq!(
        folders,
        vec!["team-ops".to_string(), "team-ops-2".to_string()]
    );
    assert!(tmp.path().join("calendars/team-ops").exists());
    assert!(tmp.path().join("calendars/team-ops-2").exists());
}

#[tokio::test]
async fn add_calendar_rejects_empty_name() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_filesystem_collection(&tmp));

    let router = make_calendars_api_router(state);
    let (status, resp) = post_query(
        router,
        &format!("/api/calendars/addcal?col_id={COL_ID}&name=%20%20"),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        resp.contains("Please enter a calendar name!"),
        "expected validation error, got:\n{resp}"
    );
}

#[tokio::test]
async fn calop_toggle_flips_enabled_flag_back_and_forth() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_collection_with_calendar(&tmp));

    {
        let locked = state.lock().await;
        let cal = locked
            .settings()
            .collections()
            .get(COL_ID)
            .unwrap()
            .all_calendars()
            .get(CAL_ID)
            .unwrap();
        assert!(cal.enabled());
    }

    let router1 = make_calendars_api_router(state.clone());
    let (status1, resp1) = post_query(
        router1,
        &format!("/api/calendars/calop?col_id={COL_ID}&cal_id={CAL_ID}&op=Toggle"),
    )
    .await;

    assert_eq!(status1, StatusCode::OK, "unexpected body:\n{resp1}");
    assert_eq!(resp1, "null");

    {
        let locked = state.lock().await;
        let cal = locked
            .settings()
            .collections()
            .get(COL_ID)
            .unwrap()
            .all_calendars()
            .get(CAL_ID)
            .unwrap();
        assert!(!cal.enabled());
    }

    let router2 = make_calendars_api_router(state.clone());
    let (status2, resp2) = post_query(
        router2,
        &format!("/api/calendars/calop?col_id={COL_ID}&cal_id={CAL_ID}&op=Toggle"),
    )
    .await;

    assert_eq!(status2, StatusCode::OK, "unexpected body:\n{resp2}");
    assert_eq!(resp2, "null");

    let locked = state.lock().await;
    let cal = locked
        .settings()
        .collections()
        .get(COL_ID)
        .unwrap()
        .all_calendars()
        .get(CAL_ID)
        .unwrap();
    assert!(cal.enabled());
}

#[tokio::test]
async fn calop_delete_removes_calendar_from_settings() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_collection_with_calendar(&tmp));

    let router = make_calendars_api_router(state.clone());
    let (status, resp) = post_query(
        router,
        &format!("/api/calendars/calop?col_id={COL_ID}&cal_id={CAL_ID}&op=Delete"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{resp}");
    assert_eq!(resp, "null");

    let locked = state.lock().await;
    assert!(
        locked
            .settings()
            .collections()
            .get(COL_ID)
            .unwrap()
            .all_calendars()
            .is_empty()
    );
}

#[tokio::test]
async fn savecal_updates_existing_calendar_settings() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_collection_with_calendar(&tmp));
    let renamed_folder = format!("{CAL_ID}-renamed");

    let body = encode_form(&[
        ("name", "Renamed Calendar"),
        ("folder", &renamed_folder),
        ("bgcolor", "#123456"),
        ("fgcolor", "#abcdef"),
        ("ev_types[]", "Event"),
        ("ev_types[]", "Todo"),
        ("alarm_type", "Calendar"),
        ("alarms[trigger]", "NONE"),
        ("alarms[duration]", "1"),
        ("alarms[durunit]", "Minutes"),
        ("alarms[durtype]", "BeforeStart"),
    ]);

    let router = make_calendars_api_router(state.clone());
    let (status, resp) = post(
        router,
        &format!("/api/calendars/savecal?col_id={COL_ID}&cal_id={CAL_ID}"),
        &body,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{resp}");
    assert_eq!(resp, "null");

    let locked = state.lock().await;
    let cal = locked
        .settings()
        .collections()
        .get(COL_ID)
        .unwrap()
        .all_calendars()
        .get(CAL_ID)
        .unwrap();
    assert_eq!(cal.name(), "Renamed Calendar");
    assert_eq!(cal.folder(), &renamed_folder);
    assert_eq!(cal.bgcolor(), "#123456");
    assert_eq!(cal.fgcolor(), "#abcdef");
    assert_eq!(cal.types(), &[CalCompType::Event, CalCompType::Todo]);
    assert!(matches!(cal.alarms(), CalendarAlarmType::Calendar));
}

#[tokio::test]
async fn savecal_creates_new_calendar_entry() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_filesystem_collection(&tmp));

    let body = encode_form(&[
        ("name", "Created Calendar"),
        ("folder", "created-cal"),
        ("bgcolor", "#112233"),
        ("fgcolor", "#445566"),
        ("alarm_type", "Calendar"),
        ("alarms[trigger]", "NONE"),
        ("alarms[duration]", "1"),
        ("alarms[durunit]", "Minutes"),
        ("alarms[durtype]", "BeforeStart"),
    ]);

    let router = make_calendars_api_router(state.clone());
    let (status, resp) = post(
        router,
        &format!("/api/calendars/savecal?col_id={COL_ID}&cal_id=created"),
        &body,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{resp}");
    assert_eq!(resp, "null");

    let locked = state.lock().await;
    let cal = locked
        .settings()
        .collections()
        .get(COL_ID)
        .unwrap()
        .all_calendars()
        .get("created")
        .expect("created calendar");
    assert!(cal.enabled());
    assert_eq!(cal.name(), "Created Calendar");
    assert_eq!(cal.folder(), "created-cal");
    assert_eq!(cal.types(), &[] as &[CalCompType]);
}

#[tokio::test]
async fn syncop_discover_collection_succeeds_for_filesystem_backend() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_collection_with_calendar(&tmp));

    let router = make_calendars_api_router(state);
    let (status, body) = post_query(
        router,
        &format!("/api/calendars/syncop?op[type]=DiscoverCollection&op[data][col_id]={COL_ID}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");

    let json = response_json(&body);
    assert_eq!(json["changed"], false);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": false})
    );
    assert_eq!(json["calendars"][CAL_ID], false);
    assert!(json["date"].as_str().is_some_and(|s| !s.is_empty()));
}

#[tokio::test]
async fn syncop_sync_collection_detects_new_filesystem_event() {
    let tmp = TempDir::new().expect("tempdir");
    let calendars_path = tmp.path().join("calendars");
    let cal_dir = calendars_path.join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).expect("create calendar");
    let (state, _config) = make_state_from_col(make_collection_with_calendar(&tmp));
    std::fs::write(
        cal_dir.join("sync-added.ics"),
        "BEGIN:VCALENDAR\r\n\
         BEGIN:VEVENT\r\n\
         UID:sync-added\r\n\
         DTSTAMP:20260101T000000Z\r\n\
         DTSTART;TZID=Europe/Berlin:20260415T090000\r\n\
         DTEND;TZID=Europe/Berlin:20260415T100000\r\n\
         SUMMARY:Synced Event\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n",
    )
    .unwrap();

    let router = make_calendars_api_router(state.clone());
    let (status, body) = post_query(
        router,
        &format!("/api/calendars/syncop?op[type]=SyncCollection&op[data][col_id]={COL_ID}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");

    let json = response_json(&body);
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": true})
    );

    let locked = state.lock().await;
    assert!(locked.store().file_by_id("sync-added").is_some());
}

#[tokio::test]
async fn syncop_reload_collection_succeeds_for_filesystem_backend() {
    let tmp = TempDir::new().expect("tempdir");
    let (state, _config) = make_state_from_col(make_collection_with_calendar(&tmp));

    let router = make_calendars_api_router(state);
    let (status, body) = post_query(
        router,
        &format!("/api/calendars/syncop?op[type]=ReloadCollection&op[data][col_id]={COL_ID}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body:\n{body}");

    let json = response_json(&body);
    assert_eq!(json["changed"], false);
    assert_eq!(
        json["collections"][COL_ID],
        serde_json::json!({"Success": false})
    );
}
