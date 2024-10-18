use anyhow::anyhow;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::parser::Property;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CalDate {
    Date(NaiveDate),
    DateTime(CalDateTime),
}

impl Default for CalDate {
    fn default() -> Self {
        Self::Date(NaiveDate::default())
    }
}

impl CalDate {
    pub fn as_start_with_tz(&self, tz: &Tz) -> DateTime<Tz> {
        match self {
            Self::Date(date) => tz
                .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                .unwrap(),
            Self::DateTime(datetime) => datetime.with_tz(tz),
        }
    }

    pub fn as_end_with_tz(&self, tz: &Tz) -> DateTime<Tz> {
        match self {
            Self::Date(date) => {
                let next_day = tz
                    .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                    .unwrap();
                next_day - Duration::seconds(1)
            }
            Self::DateTime(datetime) => datetime.with_tz(tz),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CalDateTime {
    Floating(NaiveDateTime),
    Utc(DateTime<Utc>),
    Timezone(NaiveDateTime, String),
}

impl CalDateTime {
    pub fn with_tz(&self, tz: &Tz) -> DateTime<Tz> {
        match self {
            Self::Utc(dt) => dt.with_timezone(tz),
            Self::Timezone(dt, tzid) => {
                let date_tz = if let Ok(date_tz) = tzid.parse::<Tz>() {
                    date_tz
                } else {
                    // we fall back to UTC for all weird values that we see
                    Tz::UTC
                };
                date_tz.from_local_datetime(dt).unwrap().with_timezone(tz)
            }
            Self::Floating(dt) => {
                // TODO that's certainly not correct
                let local = Local.from_utc_datetime(dt);
                local.with_timezone(tz)
            }
        }
    }
}

impl TryFrom<Property> for CalDate {
    type Error = anyhow::Error;

    fn try_from(prop: Property) -> Result<Self, Self::Error> {
        let datetime = prop.value();
        if datetime.len() < 8 {
            return Err(anyhow!("Malformed date: {}", datetime));
        }

        let year = datetime[0..4].parse::<i32>()?;
        let month = datetime[4..6].parse::<u32>()?;
        let day = datetime[6..8].parse::<u32>()?;

        if datetime.len() == 8 || prop.has_param_value("VALUE", "DATE") {
            let date = NaiveDate::from_ymd_opt(year, month, day)
                .ok_or_else(|| anyhow!("Invalid date: {datetime}"))?;
            return Ok(CalDate::Date(date));
        }

        if datetime.len() < 15 || &datetime[8..9] != "T" {
            return Err(anyhow!("Malformed datetime: {}", datetime));
        }

        let hour = datetime[9..11].parse::<u32>()?;
        let min = datetime[11..13].parse::<u32>()?;
        let sec = datetime[13..15].parse::<u32>()?;

        let date = NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|d| d.and_hms_opt(hour, min, sec))
            .ok_or_else(|| anyhow!("Invalid datetime: {datetime}"))?;

        let res = if let Some(tz) = prop.param("TZID") {
            CalDateTime::Timezone(date, tz.value().clone())
        } else if datetime.ends_with('Z') {
            CalDateTime::Utc(date.and_utc())
        } else {
            CalDateTime::Floating(date)
        };
        Ok(CalDate::DateTime(res))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_date() {
        let prop = "DUE:19990506".parse::<Property>().unwrap();
        let date: CalDate = prop.try_into().unwrap();
        assert_eq!(
            date,
            CalDate::Date(NaiveDate::from_ymd_opt(1999, 5, 6).unwrap())
        );
    }

    #[test]
    fn date_with_value() {
        let prop = "DUE;VALUE=DATE:20041030".parse::<Property>().unwrap();
        let date: CalDate = prop.try_into().unwrap();
        assert_eq!(
            date,
            CalDate::Date(NaiveDate::from_ymd_opt(2004, 10, 30).unwrap())
        );
    }

    #[test]
    fn datetime_tz() {
        let prop = "DTSTART;TZID=Europe/Berlin:20040102T081000"
            .parse::<Property>()
            .unwrap();
        let date: CalDate = prop.try_into().unwrap();

        let expected = NaiveDate::from_ymd_opt(2004, 1, 2)
            .and_then(|d| d.and_hms_opt(8, 10, 0))
            .unwrap();
        assert_eq!(
            date,
            CalDate::DateTime(CalDateTime::Timezone(expected, "Europe/Berlin".to_string()))
        );
    }

    #[test]
    fn datetime_utc() {
        let prop = "DTSTART:20241231T125622Z".parse::<Property>().unwrap();
        let date: CalDate = prop.try_into().unwrap();

        let expected = NaiveDate::from_ymd_opt(2024, 12, 31)
            .and_then(|d| d.and_hms_opt(12, 56, 22))
            .unwrap();
        assert_eq!(
            date,
            CalDate::DateTime(CalDateTime::Utc(expected.and_utc()))
        );
    }

    #[test]
    fn datetime_floating() {
        let prop = "DTSTART:18900622T002310".parse::<Property>().unwrap();
        let date: CalDate = prop.try_into().unwrap();

        let expected = NaiveDate::from_ymd_opt(1890, 6, 22)
            .and_then(|d| d.and_hms_opt(0, 23, 10))
            .unwrap();
        assert_eq!(date, CalDate::DateTime(CalDateTime::Floating(expected)));
    }
}
