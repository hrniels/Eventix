// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared date ranges and navigation for calendar views and date pickers.

use chrono::{Datelike, Duration, Months, NaiveDate};

/// Returns the Monday of the week containing `date`, if representable.
pub(crate) fn week_start(date: NaiveDate) -> Option<NaiveDate> {
    date.checked_sub_signed(Duration::days(date.weekday().num_days_from_monday().into()))
}

/// Moves by calendar months, clamping the day to the destination month's last day.
///
/// Returns `None` if the destination is outside the supported date range.
pub(crate) fn shift_month(date: NaiveDate, months: i32) -> Option<NaiveDate> {
    if months < 0 {
        date.checked_sub_months(Months::new(months.unsigned_abs()))
    } else {
        date.checked_add_months(Months::new(months as u32))
    }
}

/// Returns the Monday-aligned visible range, with an exclusive end.
///
/// Includes a trailing week when the month ends Sunday.
/// Returns `None` if the padded range exceeds the supported date range.
pub(crate) fn month_range(date: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let month = date.with_day(1)?;
    let next = shift_month(month, 1)?;
    let start = week_start(month)?;
    let end = next.checked_add_signed(Duration::days(
        7 - i64::from(next.weekday().num_days_from_monday()),
    ))?;
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, NaiveDate};

    use super::{month_range, shift_month, week_start};

    #[test]
    fn padded_months_cover_leap_days_and_year_boundaries() {
        for (year, month, first, last, count) in [
            (2024, 2, (2024, 1, 29), (2024, 3, 4), 35),
            (2026, 2, (2026, 1, 26), (2026, 3, 2), 35),
            (2026, 5, (2026, 4, 27), (2026, 6, 8), 42),
            (2026, 12, (2026, 11, 30), (2027, 1, 4), 35),
        ] {
            let date = NaiveDate::from_ymd_opt(year, month, 15).unwrap();
            let (start, end) = month_range(date).unwrap();
            assert_eq!(
                start,
                NaiveDate::from_ymd_opt(first.0, first.1, first.2).unwrap()
            );
            assert_eq!(
                end,
                NaiveDate::from_ymd_opt(last.0, last.1, last.2).unwrap()
            );
            assert_eq!((end - start).num_days(), count);
            assert_eq!(start.weekday(), chrono::Weekday::Mon);
            assert_eq!(end.weekday(), chrono::Weekday::Mon);
        }
        assert!(month_range(NaiveDate::MAX).is_none());
    }

    #[test]
    fn month_navigation_clamps_days_and_crosses_years() {
        let jan = NaiveDate::from_ymd_opt(2024, 1, 31).unwrap();
        assert_eq!(shift_month(jan, 1), NaiveDate::from_ymd_opt(2024, 2, 29));
        assert_eq!(shift_month(jan, -1), NaiveDate::from_ymd_opt(2023, 12, 31));
        assert!(shift_month(NaiveDate::MAX, 1).is_none());
    }

    #[test]
    fn week_start_preserves_monday_and_handles_year_and_date_boundaries() {
        let monday = NaiveDate::from_ymd_opt(2026, 12, 28).unwrap();
        assert_eq!(week_start(monday), Some(monday));
        assert_eq!(
            week_start(NaiveDate::from_ymd_opt(2027, 1, 3).unwrap()),
            Some(monday)
        );
        assert!(week_start(NaiveDate::MIN).is_none());
    }
}
