use anyhow::{anyhow, Context};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use ical::objects::EventLike;
use std::{str::FromStr, sync::Arc, time::SystemTime};
use tokio::sync::MutexGuard;

use crate::state::State;

pub fn parse_human_date(date: Option<String>, timezone: &Tz) -> anyhow::Result<NaiveDate> {
    let now = Utc::now().with_timezone(timezone).naive_local().date();
    match date {
        Some(s) if s.starts_with('w') => {
            let week = s[1..]
                .parse()
                .context(format!("Invalid week: {}", &s[1..]))?;
            let date = NaiveDate::from_ymd_opt(now.year(), 1, 1).unwrap();
            Ok(date + Duration::weeks(week))
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
            Ok(NaiveDate::from_str(&format!("{}-01", s))
                .context(format!("Invalid month: {}", s))?)
        }
        _ => Ok(now),
    }
}

pub fn system_time_stamp(systime: SystemTime) -> u128 {
    let duration_since_epoch = systime.duration_since(SystemTime::UNIX_EPOCH).unwrap();
    duration_since_epoch.as_nanos()
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
        .email()
        .map(|e| e.address());
    ev.is_owned_by(user_mail)
}
