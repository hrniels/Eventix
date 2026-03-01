//! Utility functions.

use std::fmt::{Debug, Display};

use formatx::formatx;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Weekday};
use chrono_tz::Tz;

use crate::objects::CalLocale;

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

/// Returns the start of the week from given date.
///
/// That is, this function walks back to the beginning of the week, keeping the time as is, even if
/// DST changes during that time period.
pub fn week_start(date: DateTime<Tz>, first_day: Option<Weekday>) -> DateTime<Tz> {
    let day_of_week = match first_day {
        Some(wkst) => date.weekday().days_since(wkst),
        _ => date.weekday().num_days_from_monday(),
    };
    if date.day() > day_of_week {
        date.with_day(date.day() - day_of_week).unwrap()
    } else {
        let (pyear, pmonth) = prev_month(date.year(), date.month());
        let days = month_days(pyear, pmonth);
        let day = days - (day_of_week - date.day());
        let naive_dt = NaiveDate::from_ymd_opt(pyear, pmonth, day)
            .unwrap()
            .and_time(date.time());
        date.timezone().from_local_datetime(&naive_dt).unwrap()
    }
}

/// Returns the end of the week from given date.
///
/// That is, this function walks forward to the end of the week, keeping the time as is, even if
/// DST changes during that time period.
pub fn week_end(date: DateTime<Tz>, first_day: Option<Weekday>) -> DateTime<Tz> {
    let day_of_week = match first_day {
        Some(wkst) => date.weekday().days_since(wkst),
        _ => date.weekday().num_days_from_monday(),
    };
    let days = month_days(date.year(), date.month());
    let diff = 7 - day_of_week - 1;
    if date.day() + diff <= days {
        date.with_day(date.day() + diff).unwrap()
    } else {
        let (nyear, nmonth) = next_month(date.year(), date.month());
        let day = diff - (days - date.day());
        let naive_dt = NaiveDate::from_ymd_opt(nyear, nmonth, day)
            .unwrap()
            .and_time(date.time());
        date.timezone().from_local_datetime(&naive_dt).unwrap()
    }
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
/// assert_eq!(eventix_ical::util::year_days(2025), 365);
/// assert_eq!(eventix_ical::util::year_days(2020), 366);
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
/// assert_eq!(eventix_ical::util::month_days(2025, 4), 30);
/// assert_eq!(eventix_ical::util::month_days(2020, 2), 29);
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
/// assert_eq!(eventix_ical::util::prev_month(2025, 4), (2025, 3));
/// assert_eq!(eventix_ical::util::prev_month(2025, 1), (2024, 12));
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
/// assert_eq!(eventix_ical::util::next_month(2025, 4), (2025, 5));
/// assert_eq!(eventix_ical::util::next_month(2025, 12), (2026, 1));
/// ```
pub fn next_month(year: i32, month: u32) -> (i32, u32) {
    match month {
        12 => (year + 1, 1),
        m => (year, m + 1),
    }
}

/// Returns a human representation of the given list.
///
/// The method will insert "," and "and" between the items as necessary. Each item needs to
/// implement [`Display`].
///
/// # Examples
///
/// ```
/// let locale = eventix_ical::objects::CalLocaleEn;
/// assert_eq!(eventix_ical::util::human_list(&[1, 2, 3], &locale), String::from("1, 2, and 3"));
/// ```
pub fn human_list<T>(objs: &[T], locale: &dyn CalLocale) -> String
where
    T: Display + Debug,
{
    if objs.len() > 1 {
        let start = itertools::join(objs.iter().take(objs.len() - 1), ", ");
        if objs.len() > 2 {
            formatx!(locale.translate("{}, and {}"), start, objs.last().unwrap()).unwrap()
        } else {
            formatx!(locale.translate("{} and {}"), start, objs.last().unwrap()).unwrap()
        }
    } else {
        itertools::join(objs.iter(), ", ")
    }
}

pub(crate) fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }

        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(';') => out.push(';'),
            Some(',') => out.push(','),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

pub(crate) fn split_escaped_commas(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut buf = String::new();
    let mut escaped = false;

    for c in value.chars() {
        if escaped {
            buf.push('\\');
            buf.push(c);
            escaped = false;
            continue;
        }

        if c == '\\' {
            escaped = true;
            continue;
        }

        if c == ',' {
            items.push(unescape_text(&buf));
            buf.clear();
            continue;
        }

        buf.push(c);
    }

    if escaped {
        buf.push('\\');
    }

    items.push(unescape_text(&buf));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::TimeZone;

    #[test]
    fn week_start_basics() {
        let tz = chrono_tz::Europe::Berlin;
        let date = tz.with_ymd_and_hms(2025, 3, 1, 10, 0, 0).unwrap();
        let expected = tz.with_ymd_and_hms(2025, 2, 24, 10, 0, 0).unwrap();
        assert_eq!(week_start(date, Some(chrono::Weekday::Mon)), expected);
        assert_eq!((date - expected).num_hours(), 5 * 24);
    }

    #[test]
    fn week_start_with_dst_change() {
        let tz = chrono_tz::Europe::Berlin;
        let date = tz.with_ymd_and_hms(2025, 3, 30, 10, 0, 0).unwrap();
        let expected = tz.with_ymd_and_hms(2025, 3, 24, 10, 0, 0).unwrap();
        assert_eq!(week_start(date, Some(chrono::Weekday::Mon)), expected);
        assert_eq!((date - expected).num_hours(), 6 * 24 - 1);
    }

    #[test]
    fn week_end_basics() {
        let tz = chrono_tz::Europe::Berlin;
        let date = tz.with_ymd_and_hms(2024, 12, 30, 10, 0, 0).unwrap();
        let expected = tz.with_ymd_and_hms(2025, 1, 5, 10, 0, 0).unwrap();
        assert_eq!(week_end(date, Some(chrono::Weekday::Mon)), expected);
        assert_eq!((expected - date).num_hours(), 6 * 24);
    }

    #[test]
    fn week_end_with_dst_change() {
        let tz = chrono_tz::Europe::Berlin;
        let date = tz.with_ymd_and_hms(2025, 3, 24, 10, 0, 0).unwrap();
        let expected = tz.with_ymd_and_hms(2025, 3, 30, 10, 0, 0).unwrap();
        assert_eq!(week_end(date, Some(chrono::Weekday::Mon)), expected);
        assert_eq!((expected - date).num_hours(), 6 * 24 - 1);
    }
}
