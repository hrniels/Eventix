//! This module provides utility functions.

use std::fmt::{Display, Write};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Weekday};
use chrono_tz::Tz;

/// Returns true if the given date ranges overlap.
pub fn date_ranges_overlap(
    start1: DateTime<Tz>,
    end1: DateTime<Tz>,
    start2: DateTime<Tz>,
    end2: DateTime<Tz>,
) -> bool {
    if start1 >= start2 && start1 < end2 {
        return true;
    }
    if end1 > start2 && end1 <= end2 {
        return true;
    }
    if start1 < start2 && end1 > end2 {
        return true;
    }
    false
}

/// Returns a [`NaiveDate`] for the nth weekday from the start of the month of given date.
///
/// For example, if `date` is 2025-01-10, `day` is Wednesday, and `n` is 2, the method will return
/// the date of the second Wednesday in January 2025, which is 2025-01-08.
pub fn nth_weekday_of_month_front(date: DateTime<Tz>, day: Weekday, n: u8) -> Option<NaiveDate> {
    NaiveDate::from_weekday_of_month_opt(date.year(), date.month(), day, n)
}

/// Returns a [`NaiveDate`] for the nth weekday from the end of the month of given date.
///
/// For example, if `date` is 2025-01-10, `day` is Wednesday, and `n` is 2, the method will return
/// the date of the second to last Wednesday in January 2025, which is 2025-01-22.
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

/// Returns a [`NaiveDate`] for the nth weekday from the start of the year of given date.
///
/// For example, if `date` is 2025-01-10, `day` is Wednesday, and `n` is 10, the method will return
/// the date of the tenth Wednesday in 2025, which is 2025-03-05.
pub fn nth_weekday_of_year_front(date: DateTime<Tz>, day: Weekday, n: u8) -> Option<NaiveDate> {
    let year_start = NaiveDate::from_ymd_opt(date.year(), 1, 1)?;
    let first_weekday = year_start.weekday();
    let first_to_dow = (7 + day.number_from_monday() - first_weekday.number_from_monday()) % 7;
    let day = (n - 1) as u32 * 7 + first_to_dow;
    Some(year_start + Duration::days(day as i64))
}

/// Returns a [`NaiveDate`] for the nth weekday from the end of the year of given date.
///
/// For example, if `date` is 2025-01-10, `day` is Wednesday, and `n` is 10, the method will return
/// the date of the tenth to last Wednesday in 2025, which is 2025-10-29.
pub fn nth_weekday_of_year_back(date: DateTime<Tz>, day: Weekday, n: u8) -> Option<NaiveDate> {
    let year_end = NaiveDate::from_ymd_opt(date.year(), 12, 31)?;
    let last_weekday = year_end.weekday();
    let first_to_dow = (7 + last_weekday.number_from_monday() - day.number_from_monday()) % 7;
    let day = (n - 1) as u32 * 7 + first_to_dow;
    Some(year_end - Duration::days(day as i64))
}

/// Returns the day number in the year of given date.
///
/// For example, if `date` is 2025-02-20, this is the 51th day of the year 2025.
pub fn year_day(date: DateTime<Tz>) -> u32 {
    date.date_naive()
        .signed_duration_since(NaiveDate::from_ymd_opt(date.year() - 1, 12, 31).unwrap())
        .num_days() as u32
}

/// Returns the number of days in the given year.
///
/// # Examples
///
/// ```
/// assert_eq!(ical::util::year_days(2025), 365);
/// assert_eq!(ical::util::year_days(2020), 366);
/// ```
pub fn year_days(year: i32) -> u32 {
    NaiveDate::from_ymd_opt(year + 1, 1, 1)
        .unwrap()
        .signed_duration_since(NaiveDate::from_ymd_opt(year, 1, 1).unwrap())
        .num_days() as u32
}

/// Returns the number of days in the given month.
///
/// # Examples
///
/// ```
/// assert_eq!(ical::util::month_days(2025, 4), 30);
/// assert_eq!(ical::util::month_days(2020, 2), 29);
/// ```
pub fn month_days(year: i32, month: u32) -> u32 {
    let (nyear, nmonth) = next_month(year, month);
    NaiveDate::from_ymd_opt(nyear, nmonth, 1)
        .unwrap()
        .signed_duration_since(NaiveDate::from_ymd_opt(year, month, 1).unwrap())
        .num_days() as u32
}

/// Returns the previous month of the given month as a pair of year and month.
///
/// # Examples
///
/// ```
/// assert_eq!(ical::util::prev_month(2025, 4), (2025, 3));
/// assert_eq!(ical::util::prev_month(2025, 1), (2024, 12));
/// ```
pub fn prev_month(year: i32, month: u32) -> (i32, u32) {
    match month {
        1 => (year - 1, 12),
        m => (year, m - 1),
    }
}

/// Returns the next month of the given month as a pair of year and month.
///
/// # Examples
///
/// ```
/// assert_eq!(ical::util::next_month(2025, 4), (2025, 5));
/// assert_eq!(ical::util::next_month(2025, 12), (2026, 1));
/// ```
pub fn next_month(year: i32, month: u32) -> (i32, u32) {
    match month {
        12 => (year + 1, 1),
        m => (year, m + 1),
    }
}

/// Returns the ordinal number for `n`.
///
/// # Examples
///
/// ```
/// assert_eq!(ical::util::nth(2), String::from("2nd"));
/// assert_eq!(ical::util::nth(10), String::from("10th"));
/// ```
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

/// Returns a human representation of the given list.
///
/// The method will insert "," and "and" between the items as necessary. Each item needs to
/// implement [`Display`].
///
/// # Examples
///
/// ```
/// assert_eq!(ical::util::human_list(&[1, 2, 3]), String::from("1, 2, and 3"));
/// ```
pub fn human_list<T>(objs: &[T]) -> String
where
    T: Display,
{
    if objs.len() > 1 {
        let start = itertools::join(objs.iter().take(objs.len() - 1), ", ");
        if objs.len() > 2 {
            format!("{}, and {}", start, objs.last().unwrap())
        } else {
            format!("{} and {}", start, objs.last().unwrap())
        }
    } else {
        itertools::join(objs.iter(), ", ")
    }
}
