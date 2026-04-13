// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod common;

use chrono::NaiveDateTime;
use eventix_ical::objects::{
    CalDate, CalDateTime, CalRRuleFreq, CalRelated, CalRole, CalTrigger, EventLike,
};
use tempfile::TempDir;

use common::{
    CAL_ID, assert_error, assert_no_ics, assert_success, encode_form, first_component, make_router,
    make_state, merge_fields, post, read_created_ics,
};

// --- Helpers specific to create-event tests ---

/// Returns the set of form fields that every create-event POST must include to satisfy the
/// `CompNew` struct's required fields.
///
/// Individual tests add or override entries on top of this baseline using `merge_fields`.
fn base_event_fields<'a>() -> Vec<(&'a str, &'a str)> {
    vec![
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

// --- Timed one-off events ---

/// A basic timed event: summary, start, end, timezone. Verifies SUMMARY, DTSTART, DTEND, and that
/// no RRULE or VALARM is produced.
#[tokio::test]
async fn timed_event_basic() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Team meeting"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    assert_eq!(comp.summary(), Some(&"Team meeting".to_string()));
    assert!(comp.rrule().is_none(), "expected no RRULE");
    assert!(comp.alarms().is_none(), "expected no VALARM");

    // Verify start and end are timed (not all-day) dates.
    match comp.start().unwrap() {
        CalDate::DateTime(_) => {}
        other => panic!("expected DTSTART as datetime, got {:?}", other),
    }
    match comp.end_or_due().unwrap() {
        CalDate::DateTime(_) => {}
        other => panic!("expected DTEND as datetime, got {:?}", other),
    }
}

/// A timed event with location and description populated.
#[tokio::test]
async fn timed_event_with_location_description() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Off-site workshop"),
            ("location", "Room 42"),
            ("description", "Bring your laptop"),
            ("start_end[from][date]", "2026-05-01"),
            ("start_end[from][time]", "14:00"),
            ("start_end[to][date]", "2026-05-01"),
            ("start_end[to][time]", "16:30"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    assert_eq!(comp.summary(), Some(&"Off-site workshop".to_string()));
    assert_eq!(comp.location(), Some(&"Room 42".to_string()));
    assert_eq!(comp.description(), Some(&"Bring your laptop".to_string()));
}

/// A timed event with a relative alarm 30 minutes before start. Verifies that a VALARM with a
/// negative 30-minute TRIGGER is produced.
#[tokio::test]
async fn timed_event_with_relative_alarm() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Doctor appointment"),
            ("start_end[from][date]", "2026-06-10"),
            ("start_end[from][time]", "11:00"),
            ("start_end[to][date]", "2026-06-10"),
            ("start_end[to][time]", "11:30"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][trigger]", "RELATIVE"),
            ("alarm[calendar][duration]", "30"),
            ("alarm[calendar][durunit]", "Minutes"),
            ("alarm[calendar][durtype]", "BeforeStart"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let alarms = comp.alarms().expect("expected VALARM");
    assert_eq!(alarms.len(), 1);

    match alarms[0].trigger() {
        CalTrigger::Relative { related, duration } => {
            assert_eq!(*related, CalRelated::Start);
            // -30 minutes: duration is stored as negative for "before"
            assert_eq!(duration.num_minutes(), -30, "expected -PT30M trigger");
        }
        other => panic!("expected relative trigger, got {:?}", other),
    }
}

/// Missing summary: no ICS file should be created and the response should contain an error.
#[tokio::test]
async fn timed_event_missing_summary() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", ""),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "08:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "09:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

/// End time before start time: no ICS file should be created and the response should show an error.
#[tokio::test]
async fn timed_event_end_before_start() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Backwards event"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "09:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

/// Missing end time: no ICS file should be created and the response should show an error.
///
/// `to_enabled` is omitted so the end date is not enabled; for an Event that is a validation error.
#[tokio::test]
async fn timed_event_missing_end() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "No end event"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "08:00"),
            ("start_end[from_enabled]", "true"),
            // to_enabled intentionally absent → end is disabled
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

// --- All-day events ---

/// A basic all-day event. DTSTART and DTEND must be VALUE=DATE, not datetime.
///
/// For all-day events the browser omits the time fields entirely; the `all_day` checkbox state is
/// communicated by the absence of `[time]` sub-fields.
#[tokio::test]
async fn allday_event_basic() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Holiday"),
            // No time fields → all-day event
            ("start_end[from][date]", "2026-12-25"),
            ("start_end[to][date]", "2026-12-25"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    assert_eq!(comp.summary(), Some(&"Holiday".to_string()));

    // All-day events use CalDate::Date, not CalDate::DateTime.
    match comp.start().unwrap() {
        CalDate::Date(_, _) => {}
        other => panic!("expected all-day DTSTART (Date), got {:?}", other),
    }
    match comp.end_or_due().unwrap() {
        CalDate::Date(_, _) => {}
        other => panic!("expected all-day DTEND (Date), got {:?}", other),
    }
}

/// A multi-day all-day event. DTSTART and DTEND span multiple days.
#[tokio::test]
async fn allday_multi_day() {
    use chrono::NaiveDate;

    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Conference"),
            ("start_end[from][date]", "2026-09-01"),
            ("start_end[to][date]", "2026-09-03"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let start_date = match comp.start().unwrap() {
        CalDate::Date(d, _) => *d,
        other => panic!("expected Date DTSTART, got {:?}", other),
    };
    assert_eq!(start_date, NaiveDate::from_ymd_opt(2026, 9, 1).unwrap());

    // DTEND for exclusive all-day events is one day past the submitted end date.
    let end_date = match comp.end_or_due().unwrap() {
        CalDate::Date(d, _) => *d,
        other => panic!("expected Date DTEND, got {:?}", other),
    };
    // The submitted end is 2026-09-03; as an exclusive end it becomes 2026-09-04.
    assert_eq!(end_date, NaiveDate::from_ymd_opt(2026, 9, 4).unwrap());
}

/// Mixed all-day / timed (start has time, end doesn't). This should be rejected with an error.
#[tokio::test]
async fn allday_mixed_with_time() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Mixed event"),
            ("start_end[from][date]", "2026-07-10"),
            ("start_end[from][time]", "09:00"),
            // end date is present but end time is absent (still all-day) → mixed
            ("start_end[to][date]", "2026-07-10"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

// --- Recurring events ---

/// A daily recurring event with no end date. Verifies RRULE:FREQ=DAILY;INTERVAL=1.
#[tokio::test]
async fn recurring_daily_no_end() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Daily standup"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "09:15"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "DAILY"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let rrule = comp.rrule().expect("expected RRULE");
    assert_eq!(rrule.frequency(), CalRRuleFreq::Daily);
    assert_eq!(rrule.interval(), Some(1));
    assert!(rrule.count().is_none(), "expected no COUNT");
    assert!(rrule.until().is_none(), "expected no UNTIL");
}

/// A weekly event repeating on Monday and Wednesday.
///
/// The `weekly_days` field uses two-letter iCalendar weekday codes (`MO,WE,`) as produced by the
/// browser's toggle_wday() JavaScript function.
#[tokio::test]
async fn recurring_weekly_mon_wed() {
    use chrono::Weekday;

    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Gym session"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "18:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "19:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "WEEKLY"),
            // Two-letter codes as stored in the hidden input by toggle_wday()
            ("rrule[weekly_days]", "MO,WE,"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let rrule = comp.rrule().expect("expected RRULE");
    assert_eq!(rrule.frequency(), CalRRuleFreq::Weekly);

    let by_day = rrule.by_day().expect("expected BYDAY");
    let days: Vec<Weekday> = by_day.iter().map(|w| w.day()).collect();
    assert!(days.contains(&Weekday::Mon), "expected Monday in BYDAY");
    assert!(days.contains(&Weekday::Wed), "expected Wednesday in BYDAY");
    assert_eq!(days.len(), 2);
}

/// A monthly event repeating on the 15th of each month.
#[tokio::test]
async fn recurring_monthly_by_monthday() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Monthly review"),
            ("start_end[from][date]", "2026-04-15"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-15"),
            ("start_end[to][time]", "11:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "MONTHLY"),
            ("rrule[monthly_type]", "ByMonthDay"),
            ("rrule[monthly_day]", "15"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let rrule = comp.rrule().expect("expected RRULE");
    assert_eq!(rrule.frequency(), CalRRuleFreq::Monthly);

    let by_mon_day = rrule.by_mon_day().expect("expected BYMONTHDAY");
    assert_eq!(by_mon_day.len(), 1);
    assert_eq!(by_mon_day[0].num(), 15, "expected BYMONTHDAY=15");
}

/// A yearly event on the same day each year (no additional constraints).
#[tokio::test]
async fn recurring_yearly_same_day() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Birthday"),
            ("start_end[from][date]", "2026-03-14"),
            ("start_end[from][time]", "00:00"),
            ("start_end[to][date]", "2026-03-14"),
            ("start_end[to][time]", "23:59"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "YEARLY"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let rrule = comp.rrule().expect("expected RRULE");
    assert_eq!(rrule.frequency(), CalRRuleFreq::Yearly);
}

/// A yearly event on March 10th (ByMonthDay). Verifies BYMONTH=3 and BYMONTHDAY=10.
#[tokio::test]
async fn recurring_yearly_by_monthday() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Annual report"),
            ("start_end[from][date]", "2026-03-10"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-03-10"),
            ("start_end[to][time]", "10:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "YEARLY"),
            ("rrule[yearly_type]", "ByMonthDay"),
            ("rrule[yearly_day]", "10"),
            ("rrule[yearly_month_bymonthday]", "March"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let rrule = comp.rrule().expect("expected RRULE");
    assert_eq!(rrule.frequency(), CalRRuleFreq::Yearly);
    assert_eq!(
        rrule.by_month(),
        Some(&vec![3u8]),
        "expected BYMONTH=3 (March)"
    );
    let by_mon_day = rrule.by_mon_day().expect("expected BYMONTHDAY");
    assert_eq!(by_mon_day[0].num(), 10, "expected BYMONTHDAY=10");
}

/// A recurring event with a fixed COUNT. Verifies COUNT=5 is preserved.
#[tokio::test]
async fn recurring_with_count() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Five standup meetings"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "09:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "09:15"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "DAILY"),
            ("rrule[end]", "Count"),
            ("rrule[count]", "5"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let rrule = comp.rrule().expect("expected RRULE");
    assert_eq!(rrule.count(), Some(5), "expected COUNT=5");
}

/// A recurring event ending on a given UNTIL date. Verifies UNTIL is set.
#[tokio::test]
async fn recurring_with_until() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Weekly sprint"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "11:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "WEEKLY"),
            ("rrule[end]", "Until"),
            ("rrule[until]", "2026-06-30"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let rrule = comp.rrule().expect("expected RRULE");
    assert!(rrule.until().is_some(), "expected UNTIL to be set");
    assert!(
        rrule.count().is_none(),
        "expected no COUNT when UNTIL is set"
    );
}

/// Monthly ByMonthDay without specifying a day: the handler returns an error banner.
#[tokio::test]
async fn recurring_monthly_bymonthday_missing_day() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Missing day"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "11:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "MONTHLY"),
            // ByMonthDay selected but monthly_day intentionally absent
            ("rrule[monthly_type]", "ByMonthDay"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

/// Yearly ByWeekday without specifying nth position: the handler returns an error banner.
#[tokio::test]
async fn recurring_yearly_byweekday_missing_nth() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Yearly weekday"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "11:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "YEARLY"),
            // ByWeekday without yearly_nth → error
            ("rrule[yearly_type]", "ByWeekday"),
            ("rrule[yearly_month_byweekday]", "April"),
            // yearly_nth intentionally absent
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

// --- Attendees ---

/// An event with one required attendee. Verifies ORGANIZER and a REQ-PARTICIPANT ATTENDEE.
#[tokio::test]
async fn event_with_one_required_attendee() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Team lunch"),
            ("start_end[from][date]", "2026-05-10"),
            ("start_end[from][time]", "12:00"),
            ("start_end[to][date]", "2026-05-10"),
            ("start_end[to][time]", "13:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("attendees[name][0]", "Jane Doe <jane@example.com>"),
            ("attendees[role][0]", "Required"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let attendees = comp.attendees().expect("expected ATTENDEE properties");
    assert_eq!(attendees.len(), 1);
    assert_eq!(
        attendees[0].role(),
        Some(CalRole::Required),
        "expected REQ-PARTICIPANT role"
    );
    assert!(
        attendees[0].address().contains("jane@example.com"),
        "expected jane@example.com in ATTENDEE"
    );
}

/// An event with an optional attendee. Verifies OPT-PARTICIPANT role.
#[tokio::test]
async fn event_with_optional_attendee() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Optional meeting"),
            ("start_end[from][date]", "2026-05-12"),
            ("start_end[from][time]", "15:00"),
            ("start_end[to][date]", "2026-05-12"),
            ("start_end[to][time]", "16:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("attendees[name][0]", "bob@example.com"),
            ("attendees[role][0]", "Optional"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let attendees = comp.attendees().expect("expected ATTENDEE");
    assert_eq!(attendees[0].role(), Some(CalRole::Optional));
}

/// An event with two attendees. Verifies that both ATTENDEE entries are written.
#[tokio::test]
async fn event_with_two_attendees() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Project kickoff"),
            ("start_end[from][date]", "2026-05-15"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-05-15"),
            ("start_end[to][time]", "11:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("attendees[name][0]", "Alice <alice@example.com>"),
            ("attendees[role][0]", "Required"),
            ("attendees[name][1]", "Bob <bob@example.com>"),
            ("attendees[role][1]", "Optional"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let attendees = comp.attendees().expect("expected two ATTENDEEs");
    assert_eq!(attendees.len(), 2, "expected exactly 2 attendees");
}

// --- Validation: missing start ---

/// An event with the end enabled but start absent. The handler must reject it with
/// `error.start_datetime`.
#[tokio::test]
async fn timed_event_missing_start() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "No start event"),
            // from_enabled intentionally absent → start is disabled
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "10:00"),
            ("start_end[to_enabled]", "true"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

/// A recurring event with no start date. The handler must reject it with
/// `error.repeating_event_start`.
#[tokio::test]
async fn recurring_event_missing_start() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Recurring without start"),
            // No from_enabled → no start date
            ("rrule[freq]", "DAILY"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

/// Until end with no until date provided: the handler must return an error.
#[tokio::test]
async fn recurring_with_until_missing_date() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Until without date"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "11:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "DAILY"),
            ("rrule[end]", "Until"),
            // until date intentionally absent
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

/// Yearly ByMonthDay without specifying a month: the handler returns an error.
#[tokio::test]
async fn recurring_yearly_bymonthday_missing_month() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Yearly no month"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "11:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "YEARLY"),
            ("rrule[yearly_type]", "ByMonthDay"),
            ("rrule[yearly_day]", "15"),
            // yearly_month_bymonthday intentionally absent
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

/// Yearly ByMonthDay without specifying a day: the handler returns an error.
#[tokio::test]
async fn recurring_yearly_bymonthday_missing_day() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Yearly no day"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "11:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "YEARLY"),
            ("rrule[yearly_type]", "ByMonthDay"),
            ("rrule[yearly_month_bymonthday]", "June"),
            // yearly_day intentionally absent
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

/// Yearly ByWeekday without specifying a weekday: the handler returns an error.
#[tokio::test]
async fn recurring_yearly_byweekday_missing_weekday() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Yearly no weekday"),
            ("start_end[from][date]", "2026-04-20"),
            ("start_end[from][time]", "10:00"),
            ("start_end[to][date]", "2026-04-20"),
            ("start_end[to][time]", "11:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("rrule[freq]", "YEARLY"),
            ("rrule[yearly_type]", "ByWeekday"),
            ("rrule[yearly_month_byweekday]", "May"),
            ("rrule[yearly_nth]", "First"),
            // yearly_wday intentionally absent
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

// --- Absolute alarms ---

/// An absolute alarm with a specific datetime. Verifies that TRIGGER;VALUE=DATE-TIME is stored as
/// a UTC datetime in the produced VALARM.
#[tokio::test]
async fn timed_event_alarm_absolute() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Dentist"),
            ("start_end[from][date]", "2026-06-10"),
            ("start_end[from][time]", "11:00"),
            ("start_end[to][date]", "2026-06-10"),
            ("start_end[to][time]", "12:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            // Absolute alarm at 2026-06-10 09:00 Europe/Berlin = 07:00 UTC (UTC+2 in summer)
            ("alarm[calendar][trigger]", "ABSOLUTE"),
            ("alarm[calendar][datetime][date]", "2026-06-10"),
            ("alarm[calendar][datetime][time]", "09:00"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_success(&resp_body);

    let ics = read_created_ics(&cal_dir);
    let comp = first_component(&ics);
    let alarms = comp.alarms().expect("expected VALARM");
    assert_eq!(alarms.len(), 1);

    match alarms[0].trigger() {
        CalTrigger::Absolute(CalDate::DateTime(CalDateTime::Utc(dt))) => {
            let expected =
                NaiveDateTime::parse_from_str("2026-06-10 07:00:00", "%Y-%m-%d %H:%M:%S")
                    .unwrap()
                    .and_utc();
            assert_eq!(
                *dt, expected,
                "expected alarm trigger at 2026-06-10T07:00:00Z"
            );
        }
        other => panic!("expected absolute UTC trigger, got {:?}", other),
    }
}

/// An absolute alarm with no datetime specified: the handler returns an error.
#[tokio::test]
async fn timed_event_alarm_absolute_missing_datetime() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Meeting"),
            ("start_end[from][date]", "2026-06-10"),
            ("start_end[from][time]", "11:00"),
            ("start_end[to][date]", "2026-06-10"),
            ("start_end[to][time]", "12:00"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("alarm[calendar][trigger]", "ABSOLUTE"),
            // datetime fields intentionally absent → error.valid_date_time
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

// --- Timezone DST errors ---

/// Start datetime falls in the Europe/Berlin spring-forward gap (2026-03-29 02:30 does not exist).
/// The handler must reject the event with an error.
#[tokio::test]
async fn timed_event_start_in_dst_gap() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Gap event"),
            // Europe/Berlin: clocks spring forward at 02:00 → 03:00 on 2026-03-29;
            // 02:30 is a non-existent local time.
            ("start_end[from][date]", "2026-03-29"),
            ("start_end[from][time]", "02:30"),
            ("start_end[to][date]", "2026-03-29"),
            ("start_end[to][time]", "03:30"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("start_end[timezone]", "Europe/Berlin"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}

/// End datetime falls in the Europe/Berlin autumn fold (2026-10-25 02:30 is ambiguous).
/// The handler must reject the event with an error.
#[tokio::test]
async fn timed_event_end_in_dst_fold() {
    let tmp = TempDir::new().unwrap();
    let cal_dir = tmp.path().join(CAL_ID);
    std::fs::create_dir_all(&cal_dir).unwrap();
    let state = make_state(&cal_dir);
    let router = make_router(state);

    let fields = merge_fields(
        base_event_fields(),
        &[
            ("calendar", CAL_ID),
            ("summary", "Fold event"),
            // Europe/Berlin: clocks fall back at 03:00 → 02:00 on 2026-10-25;
            // 02:30 occurs twice and is ambiguous.
            ("start_end[from][date]", "2026-10-25"),
            ("start_end[from][time]", "01:30"),
            ("start_end[to][date]", "2026-10-25"),
            ("start_end[to][time]", "02:30"),
            ("start_end[from_enabled]", "true"),
            ("start_end[to_enabled]", "true"),
            ("start_end[timezone]", "Europe/Berlin"),
        ],
    );
    let body = encode_form(&fields);

    let (status, resp_body) = post(router, "/pages/items/add?ctype=Event", &body).await;
    assert_eq!(status, 200);
    assert_error(&resp_body);
    assert_no_ics(&cal_dir);
}
