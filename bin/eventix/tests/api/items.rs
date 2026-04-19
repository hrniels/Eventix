// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../helper/mod.rs"]
mod helper;

use std::path::{Path, PathBuf};

/// Writes a minimal timed VEVENT ICS file for `uid` into `cal_dir` and returns the path.
///
/// The event runs 2026-04-15 09:00–10:00 in Europe/Berlin with the given summary.
fn write_event_ics(cal_dir: &Path, uid: &str, summary: &str) -> PathBuf {
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

/// Writes a minimal timed VEVENT ICS file for `uid` in the given `tzid` and local hour range.
fn write_event_ics_in_tz(
    cal_dir: &Path,
    uid: &str,
    summary: &str,
    tzid: &str,
    start_hour: u32,
    end_hour: u32,
) -> PathBuf {
    let path = cal_dir.join(format!("{uid}.ics"));
    std::fs::write(
        &path,
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             DTSTART;TZID={tzid}:20260415T{start_hour:02}0000\r\n\
             DTEND;TZID={tzid}:20260415T{end_hour:02}0000\r\n\
             SUMMARY:{summary}\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
    path
}

/// Writes a minimal weekly recurring VEVENT ICS file for `uid` into `cal_dir` and returns the
/// path.
///
/// The event starts 2026-04-15 09:00 Europe/Berlin and repeats every Wednesday.
fn write_recurring_event_ics(cal_dir: &Path, uid: &str) -> PathBuf {
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

/// Writes a minimal all-day VEVENT ICS file for `uid` into `cal_dir` and returns the path.
///
/// The event spans 2026-04-15 (a single day, DATE value).
fn write_allday_event_ics(cal_dir: &Path, uid: &str, summary: &str) -> PathBuf {
    let path = cal_dir.join(format!("{uid}.ics"));
    std::fs::write(
        &path,
        format!(
            "BEGIN:VCALENDAR\r\n\
             BEGIN:VEVENT\r\n\
             UID:{uid}\r\n\
             DTSTAMP:20260101T000000Z\r\n\
             DTSTART;VALUE=DATE:20260415\r\n\
             DTEND;VALUE=DATE:20260416\r\n\
             SUMMARY:{summary}\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ),
    )
    .unwrap();
    path
}

#[path = "items/add.rs"]
mod add;
#[path = "items/cancel.rs"]
mod cancel;
#[path = "items/complete.rs"]
mod complete;
#[path = "items/copy.rs"]
mod copy;
#[path = "items/delete.rs"]
mod delete;
#[path = "items/details.rs"]
mod details;
#[path = "items/editalarm.rs"]
mod editalarm;
#[path = "items/occlist.rs"]
mod occlist;
#[path = "items/resize.rs"]
mod resize;
#[path = "items/respond.rs"]
mod respond;
#[path = "items/shift.rs"]
mod shift;
#[path = "items/toggle.rs"]
mod toggle;
#[path = "items/tzconvert.rs"]
mod tzconvert;
