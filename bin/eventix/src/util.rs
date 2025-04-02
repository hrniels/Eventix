use anyhow::{anyhow, Context};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use ical::objects::CalOrganizer;
use std::{str::FromStr, sync::Arc};
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

pub fn user_is_event_owner(
    dir: &Arc<String>,
    state: &MutexGuard<'_, State>,
    ev_org: Option<&CalOrganizer>,
) -> bool {
    let own_org = state.settings().calendar(dir).unwrap().build_organizer();
    is_event_owner(own_org.as_ref(), ev_org)
}

pub fn is_event_owner(own_org: Option<&CalOrganizer>, ev_org: Option<&CalOrganizer>) -> bool {
    match (ev_org, own_org) {
        (Some(ev_org), Some(own_org)) if ev_org.address() == own_org.address() => true,
        (Some(_), _) => false,
        (None, _) => true,
    }
}
