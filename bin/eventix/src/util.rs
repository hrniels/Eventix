use anyhow::{anyhow, Context};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use std::str::FromStr;

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
