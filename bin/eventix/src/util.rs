// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use eventix_ical::objects::EventLike;
use eventix_state::{EmailAccount, State};
use std::{str::FromStr, sync::Arc, time::SystemTime};
use tokio::sync::MutexGuard;

pub fn parse_human_date(date: Option<String>, timezone: &Tz) -> anyhow::Result<NaiveDate> {
    let now = Utc::now().with_timezone(timezone).naive_local().date();
    match date {
        Some(s) if s.starts_with('y') => {
            let year = s[1..]
                .parse()
                .context(format!("Parse year failed: {}", &s[1..]))?;
            NaiveDate::from_ymd_opt(year, 1, 1).context(format!("Invalid year {}", &s[1..]))
        }
        Some(s) if s.contains('w') => {
            let (s, year) = if let Some(week) = s.strip_prefix('w') {
                (week, now.year())
            } else {
                let mut parts = s.split('w');
                let year_str = parts.next().unwrap();
                let year = year_str
                    .parse()
                    .context(format!("Parse year failed: {year_str}"))?;
                (parts.next().unwrap(), year)
            };

            let week = s.parse().context(format!("Parse week failed: {s}"))?;
            let date =
                NaiveDate::from_ymd_opt(year, 1, 1).context(format!("Invalid year {year}"))?;
            Ok(if date.iso_week().week() != 1 {
                date + Duration::weeks(week)
            } else {
                date + Duration::weeks(week - 1)
            })
        }
        Some(s) if s.starts_with('m') => {
            let month = s[1..]
                .parse()
                .context(format!("Invalid month: {}", &s[1..]))?;
            Ok(NaiveDate::from_ymd_opt(now.year(), month, 1)
                .ok_or_else(|| anyhow!("Invalid date {}-{}-01", now.year(), month))?)
        }
        Some(s) if s.contains('-') => {
            if let Ok(res) = NaiveDate::from_str(&s) {
                return Ok(res);
            }
            Ok(NaiveDate::from_str(&format!("{s}-01")).context(format!("Invalid month: {s}"))?)
        }
        _ => Ok(now),
    }
}

pub fn system_time_stamp(systime: SystemTime) -> u128 {
    let duration_since_epoch = systime.duration_since(SystemTime::UNIX_EPOCH).unwrap();
    duration_since_epoch.as_nanos()
}

pub fn user_for_uid(
    state: &MutexGuard<'_, State>,
    uid: &String,
) -> anyhow::Result<Option<EmailAccount>> {
    let file = state
        .store()
        .file_by_id(uid)
        .ok_or_else(|| anyhow!("Unable to find file with uid {}", uid))?;

    Ok(state
        .settings()
        .calendar(file.directory())
        .unwrap()
        .0
        .email()
        .cloned())
}

pub fn user_is_event_owner<E: EventLike>(
    dir: &Arc<String>,
    state: &MutexGuard<'_, State>,
    ev: &E,
) -> bool {
    let user_mail = state
        .settings()
        .calendar(dir)
        .unwrap()
        .0
        .email()
        .map(|e| e.address());
    ev.is_owned_by(user_mail)
}
