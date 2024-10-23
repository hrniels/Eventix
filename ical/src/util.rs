use std::fmt::Write;

use chrono::{DateTime, Datelike, Duration, NaiveDate, Weekday};
use chrono_tz::Tz;

pub fn nth_weekday_of_month_front(date: DateTime<Tz>, day: Weekday, n: u8) -> Option<NaiveDate> {
    NaiveDate::from_weekday_of_month_opt(date.year(), date.month(), day, n)
}

pub fn nth_weekday_of_month_back(date: DateTime<Tz>, day: Weekday, n: u8) -> Option<NaiveDate> {
    let (year, month) = next_month(date.year(), date.month());
    let next_month = NaiveDate::from_ymd_opt(year, month, 1)?;
    let last = next_month.pred_opt()?;
    let last_weekday = last.weekday();
    let last_day = last.day();
    let first_to_dow = (7 + last_weekday.number_from_monday() - day.number_from_monday()) % 7;
    let day = last_day - ((n - 1) as u32 * 7 + first_to_dow);
    NaiveDate::from_ymd_opt(date.year(), date.month(), day)
}

pub fn nth_weekday_of_year_front(date: DateTime<Tz>, day: Weekday, n: u8) -> Option<NaiveDate> {
    let year_start = NaiveDate::from_ymd_opt(date.year(), 1, 1)?;
    let first_weekday = year_start.weekday();
    let first_to_dow = (7 + day.number_from_monday() - first_weekday.number_from_monday()) % 7;
    let day = (n - 1) as u32 * 7 + first_to_dow;
    Some(year_start + Duration::days(day as i64))
}

pub fn nth_weekday_of_year_back(date: DateTime<Tz>, day: Weekday, n: u8) -> Option<NaiveDate> {
    let year_end = NaiveDate::from_ymd_opt(date.year(), 12, 31)?;
    let last_weekday = year_end.weekday();
    let first_to_dow = (7 + last_weekday.number_from_monday() - day.number_from_monday()) % 7;
    let day = (n - 1) as u32 * 7 + first_to_dow;
    Some(year_end - Duration::days(day as i64))
}

pub fn year_day(date: DateTime<Tz>) -> u32 {
    date.date_naive()
        .signed_duration_since(NaiveDate::from_ymd_opt(date.year() - 1, 12, 31).unwrap())
        .num_days() as u32
}

pub fn year_days(year: i32) -> u32 {
    NaiveDate::from_ymd_opt(year + 1, 1, 1)
        .unwrap()
        .signed_duration_since(NaiveDate::from_ymd_opt(year, 1, 1).unwrap())
        .num_days() as u32
}

pub fn month_days(year: i32, month: u32) -> u32 {
    let (nyear, nmonth) = next_month(year, month);
    NaiveDate::from_ymd_opt(nyear, nmonth, 1)
        .unwrap()
        .signed_duration_since(NaiveDate::from_ymd_opt(year, month, 1).unwrap())
        .num_days() as u32
}

pub fn prev_month(year: i32, month: u32) -> (i32, u32) {
    match month {
        1 => (year - 1, 12),
        m => (year, m - 1),
    }
}

pub fn next_month(year: i32, month: u32) -> (i32, u32) {
    match month {
        12 => (year + 1, 1),
        m => (year, m + 1),
    }
}

pub fn nth(n: u64) -> String {
    let mut res = String::new();
    res.write_fmt(format_args!("{}", n)).unwrap();
    match n % 10 {
        1 if n != 11 => res.push_str("st"),
        2 if n != 12 => res.push_str("nd"),
        3 if n != 13 => res.push_str("rd"),
        _ => res.push_str("th"),
    }
    res
}

pub fn human_list<T>(objs: &[T]) -> String
where
    T: AsRef<str>,
{
    if objs.len() > 1 {
        let start = itertools::join(
            objs.iter()
                .take(objs.len() - 1)
                .map(|o| format!("{}", o.as_ref())),
            ", ",
        );
        if objs.len() > 2 {
            format!("{}, and {}", start, objs.last().unwrap().as_ref())
        } else {
            format!("{} and {}", start, objs.last().unwrap().as_ref())
        }
    } else {
        itertools::join(objs.iter().map(|o| format!("{}", o.as_ref())), ", ")
    }
}
