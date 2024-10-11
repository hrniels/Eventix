use anyhow::anyhow;
use chrono::{DateTime, Local, NaiveDate, TimeZone};
use chrono_tz::{Tz, UTC};
use ical::property::Property;
use std::str::FromStr;

mod todo;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ICalDate {
    Date(NaiveDate),
    DateTimeLocal(DateTime<Local>),
    DateTimeUtc(DateTime<Local>),
    DateTimeTz(DateTime<Local>, Tz),
}

impl Default for ICalDate {
    fn default() -> Self {
        Self::Date(NaiveDate::default())
    }
}

fn invalid_date(year: i32, mon: u32, day: u32) -> anyhow::Error {
    anyhow!("Invalid date {}{}{}", year, mon, day,)
}

fn invalid_datetime(year: i32, mon: u32, day: u32, hour: u32, min: u32, sec: u32) -> anyhow::Error {
    anyhow!(
        "Invalid date/time {}{}{} {}{}{}",
        year,
        mon,
        day,
        hour,
        min,
        sec
    )
}

impl FromStr for ICalDate {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let get_ymd = |s: &str| -> Result<(i32, u32, u32), Self::Err> {
            let year = s[0..4].parse::<i32>()?;
            let month = s[4..6].parse::<u32>()?;
            let day = s[6..8].parse::<u32>()?;
            Ok((year, month, day))
        };

        let get_time = |s: &str| -> Result<(u32, u32, u32), Self::Err> {
            let hour = s[0..2].parse::<u32>()?;
            let min = s[2..4].parse::<u32>()?;
            let sec = s[4..6].parse::<u32>()?;
            Ok((hour, min, sec))
        };

        if let Some(datetime) = s.strip_prefix("TZID=") {
            let mut parts = datetime.split(':');
            let tz = parts
                .next()
                .ok_or_else(|| anyhow!("Empty date"))?
                .parse::<Tz>()?;
            let time = parts.next().ok_or_else(|| anyhow!("Missing datetime"))?;
            let (year, mon, day) = get_ymd(&time[0..8])?;
            let (hour, min, sec) = get_time(&time[9..])?;
            let tzdate = tz
                .with_ymd_and_hms(year, mon, day, hour, min, sec)
                .single()
                .ok_or_else(|| invalid_datetime(year, mon, day, hour, min, sec))?;
            let date: DateTime<Local> = tzdate.with_timezone(&Local);
            return Ok(ICalDate::DateTimeTz(date, tz));
        }

        let (year, mon, day) = get_ymd(&s[0..8])?;

        if s.ends_with('Z') {
            let (hour, min, sec) = get_time(&s[9..])?;
            let tzdate = UTC
                .with_ymd_and_hms(year, mon, day, hour, min, sec)
                .single()
                .ok_or_else(|| invalid_datetime(year, mon, day, hour, min, sec))?;
            let date: DateTime<Local> = tzdate.with_timezone(&Local);
            return Ok(ICalDate::DateTimeUtc(date));
        }

        if s.len() > 8 {
            let (hour, min, sec) = get_time(&s[9..])?;
            let date = Local
                .with_ymd_and_hms(year, mon, day, hour, min, sec)
                .single()
                .ok_or_else(|| invalid_datetime(year, mon, day, hour, min, sec))?;
            return Ok(ICalDate::DateTimeLocal(date));
        }

        let date =
            NaiveDate::from_ymd_opt(year, mon, day).ok_or_else(|| invalid_date(year, mon, day))?;
        Ok(ICalDate::Date(date))
    }
}

impl TryFrom<&Property> for ICalDate {
    type Error = anyhow::Error;

    fn try_from(prop: &Property) -> Result<Self, Self::Error> {
        prop.value
            .as_ref()
            .ok_or_else(|| anyhow!("Missing value"))?
            .parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ical::PropertyParser;

    #[test]
    fn prop_date() {
        let mut parser = PropertyParser::from_reader(b"DUE:19990506".as_slice());
        let prop = &parser.next().unwrap().unwrap();
        let date: ICalDate = prop.try_into().unwrap();
        assert_eq!(
            date,
            ICalDate::Date(NaiveDate::from_ymd_opt(1999, 5, 6).unwrap())
        );
    }

    #[test]
    fn datetime_tz() {
        let date: ICalDate = "TZID=Europe/Berlin:20040102T081000".parse().unwrap();
        let tz: Tz = "Europe/Berlin".parse().unwrap();
        let naive_date = NaiveDate::from_ymd_opt(2004, 1, 2)
            .unwrap()
            .and_hms_opt(8, 10, 0)
            .unwrap();
        let local_date = naive_date.and_local_timezone(Local).single().unwrap();
        assert_eq!(date, ICalDate::DateTimeTz(local_date, tz));
    }

    #[test]
    fn datetime_utc() {
        let date: ICalDate = "20241231T125622Z".parse().unwrap();
        let naive_date = NaiveDate::from_ymd_opt(2024, 12, 31)
            .unwrap()
            .and_hms_opt(12, 56, 22)
            .unwrap();
        let utc_date = naive_date.and_utc();
        let local_date = utc_date.with_timezone(&Local);
        assert_eq!(date, ICalDate::DateTimeUtc(local_date));
    }

    #[test]
    fn datetime_local() {
        let date: ICalDate = "18900622T002310".parse().unwrap();
        let naive_date = NaiveDate::from_ymd_opt(1890, 6, 22)
            .unwrap()
            .and_hms_opt(0, 23, 10)
            .unwrap();
        let local_date = naive_date.and_local_timezone(Local).single().unwrap();
        assert_eq!(date, ICalDate::DateTimeLocal(local_date));
    }
}
