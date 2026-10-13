// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Toolkit-independent monthly calendar projection.

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{Datelike, Duration, Months, NaiveDate};
use eventix_locale::Locale;
use eventix_state::State;

use crate::{
    Event, TimedSegmentId,
    projection::{day_minutes, event_order, events_between},
    week_start,
};

/// One occurrence displayed in a monthly day cell.
#[derive(Clone, Debug)]
pub struct MonthEvent {
    pub event: Event,
    pub date: NaiveDate,
    /// True for all-day occurrences and interior full days of timed occurrences.
    pub full_day: bool,
}

impl MonthEvent {
    /// Returns the identity of this occurrence's displayed day.
    pub fn id(&self) -> TimedSegmentId {
        TimedSegmentId {
            occurrence: self.event.id.clone(),
            date: self.date,
        }
    }
}

/// A monthly day cell, with full-day entries before chronological timed entries.
#[derive(Clone, Debug)]
pub struct MonthDay {
    pub date: NaiveDate,
    pub current_month: bool,
    pub show_month: bool,
    pub events: Vec<MonthEvent>,
}

/// An immutable monthly snapshot including neighboring-month padding days.
pub struct MonthSnapshot {
    /// First day of the selected month.
    pub month: NaiveDate,
    /// Monday of the first visible week.
    pub start: NaiveDate,
    pub days: Vec<MonthDay>,
    pub locale: Arc<dyn Locale + Send + Sync>,
}

/// Moves by calendar months, clamping the day to the destination month's last day.
///
/// Returns `None` if the destination is outside the supported date range.
pub fn shift_month(date: NaiveDate, months: i32) -> Option<NaiveDate> {
    if months < 0 {
        date.checked_sub_months(Months::new(months.unsigned_abs()))
    } else {
        date.checked_add_months(Months::new(months as u32))
    }
}

/// Returns the Monday-aligned visible range, with an exclusive end.
///
/// Matches the HTML calendar's padding, including a trailing week when the month ends Sunday.
/// Returns `None` if the padded range exceeds the supported date range.
pub fn month_range(date: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let month = date.with_day(1)?;
    let next = shift_month(month, 1)?;
    let start = week_start(month)?;
    let end = next.checked_add_signed(Duration::days(
        7 - i64::from(next.weekday().num_days_from_monday()),
    ))?;
    Some((start, end))
}

impl MonthSnapshot {
    /// Projects visible, non-excluded events into the month containing `date`.
    ///
    /// Uses the configured timezone, preserves original occurrence details, and repeats multi-day
    /// occurrences in each applicable cell. Midnight endings remain exclusive.
    /// Returns an error if the padded month exceeds the supported date range.
    pub fn load(state: &State, date: NaiveDate) -> Result<Self> {
        let month = date.with_day(1).context("month start out of range")?;
        let (start, end) = month_range(date).context("month range out of range")?;
        let occurrences = events_between(state, start, end);
        let mut days = Vec::new();
        for offset in 0..(end - start).num_days() {
            let date = start + Duration::days(offset);
            let mut events: Vec<_> = occurrences
                .iter()
                .filter_map(|(occ, event)| {
                    day_minutes(event.start, event.end, date)?;
                    Some(MonthEvent {
                        event: event.clone(),
                        date,
                        full_day: event.all_day || occ.is_all_day_on(date),
                    })
                })
                .collect();
            events.sort_by(|a, b| {
                (!a.full_day)
                    .cmp(&!b.full_day)
                    .then_with(|| event_order(&a.event, &b.event))
            });
            days.push(MonthDay {
                date,
                current_month: date.year() == month.year() && date.month() == month.month(),
                show_month: date.day() == 1
                    || date.succ_opt().is_some_and(|d| d.month() != date.month()),
                events,
            });
        }
        Ok(Self {
            month,
            start,
            days,
            locale: state.locale(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Datelike, NaiveDate};
    use chrono_tz::Europe::Berlin;
    use eventix_ical::col::{CalDir, CalFile};
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};

    use super::{MonthSnapshot, month_range, shift_month};

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
    fn projects_day_cells_with_padding_order_exclusions_and_timezone_boundaries() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let mut state = State::new_with_timezone(Arc::new(xdg), Some(Berlin)).unwrap();
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: tmp.path().to_string_lossy().into_owned(),
        });
        let mut settings = CalendarSettings::default();
        settings.set_enabled(true);
        settings.set_name("Work".into());
        settings.set_bgcolor("#123456".into());
        collection
            .all_calendars_mut()
            .insert("cal".into(), settings);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        let id = Arc::new("cal".to_string());
        let mut dir = CalDir::new_empty(id.clone(), tmp.path().into(), "Work".into(), false);
        let events = [
            concat!(
                "BEGIN:VEVENT\nUID:padding\nDTSTART;VALUE=DATE:20260929\n",
                "DTEND;VALUE=DATE:20261002\nSUMMARY:Across months\nEND:VEVENT"
            ),
            concat!(
                "BEGIN:VEVENT\nUID:long\nDTSTART;TZID=Europe/Berlin:20261004T120000\n",
                "DTEND;TZID=Europe/Berlin:20261007T000000\nSUMMARY:Three days\nEND:VEVENT"
            ),
            concat!(
                "BEGIN:VEVENT\nUID:allday\nDTSTART;VALUE=DATE:20261005\n",
                "DTEND;VALUE=DATE:20261006\nSUMMARY:All day\nEND:VEVENT"
            ),
            concat!(
                "BEGIN:VEVENT\nUID:utc\nDTSTART:20261004T233000Z\n",
                "DTEND:20261005T003000Z\nSUMMARY:Local Monday\nEND:VEVENT"
            ),
            concat!(
                "BEGIN:VEVENT\nUID:midnight\nDTSTART;TZID=Europe/Berlin:20261005T230000\n",
                "DTEND;TZID=Europe/Berlin:20261006T000000\nEND:VEVENT"
            ),
            concat!(
                "BEGIN:VEVENT\nUID:series\nDTSTART;TZID=Europe/Berlin:20261005T090000\n",
                "DTEND;TZID=Europe/Berlin:20261005T100000\nRRULE:FREQ=DAILY;COUNT=3\n",
                "EXDATE;TZID=Europe/Berlin:20261006T090000\nSUMMARY:Series\nEND:VEVENT\n",
                "BEGIN:VEVENT\nUID:series\nRECURRENCE-ID;TZID=Europe/Berlin:20261007T090000\n",
                "DTSTART;TZID=Europe/Berlin:20261007T110000\n",
                "DTEND;TZID=Europe/Berlin:20261007T120000\nSUMMARY:Moved\nEND:VEVENT"
            ),
            concat!(
                "BEGIN:VEVENT\nUID:fold\nDTSTART:20261025T003000Z\n",
                "DTEND:20261025T013000Z\nSUMMARY:Repeated hour\nEND:VEVENT"
            ),
            concat!(
                "BEGIN:VTODO\nUID:todo\nDTSTART;VALUE=DATE:20261005\n",
                "DUE;VALUE=DATE:20261006\nEND:VTODO"
            ),
        ];
        for (index, event) in events.iter().enumerate() {
            let calendar = format!("BEGIN:VCALENDAR\nVERSION:2.0\n{event}\nEND:VCALENDAR\n")
                .parse()
                .unwrap();
            dir.add_file(CalFile::new(
                id.clone(),
                tmp.path().join(format!("{index}.ics")),
                calendar,
            ))
            .unwrap();
        }
        state.store_mut().add(dir);
        let date = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        let month = MonthSnapshot::load(&state, date).unwrap();
        assert_eq!(month.month, date.with_day(1).unwrap());
        assert_eq!(month.start, NaiveDate::from_ymd_opt(2026, 9, 28).unwrap());
        assert_eq!(month.days.len(), 35);
        assert!(!month.days[1].current_month);
        assert_eq!(month.days[1].events[0].event.id.uid, "padding");
        assert!(month.days[3].current_month && month.days[3].show_month);
        assert_eq!(month.days[3].events[0].event.id.uid, "padding");
        assert!(month.days[4].events.is_empty(), "DATE end is exclusive");
        let monday = &month.days[7];
        assert_eq!(monday.date, date);
        assert_eq!(
            monday
                .events
                .iter()
                .map(|e| e.event.id.uid.as_str())
                .collect::<Vec<_>>(),
            ["long", "allday", "utc", "series", "midnight"]
        );
        assert!(monday.events[0].full_day && monday.events[1].full_day);
        assert!(!monday.events[2].full_day);
        assert_eq!(
            month.days[8].events.len(),
            1,
            "midnight ends and EXDATE do not leak"
        );
        assert!(
            !month.days[8].events[0].full_day,
            "last logical day retains end time"
        );
        let moved = &month.days[9].events[0].event;
        assert!(moved.exception && moved.recurrent);
        assert_eq!(moved.summary, "Moved");
        assert_eq!(moved.background, "#123456");
        assert_eq!(moved.calendar, "Work");
        assert!(moved.details.date.contains("11:00"));
        let fold = month
            .days
            .iter()
            .flat_map(|d| &d.events)
            .find(|e| e.event.id.uid == "fold")
            .unwrap();
        assert_eq!(fold.date, NaiveDate::from_ymd_opt(2026, 10, 25).unwrap());
        assert_eq!((fold.event.end - fold.event.start).num_hours(), 1);
        let refreshed = MonthSnapshot::load(&state, date).unwrap();
        assert_eq!(monday.events[3].id(), refreshed.days[7].events[3].id());
        assert_ne!(month.days[6].events[0].id(), monday.events[0].id());
        state.misc_mut().toggle_calendar(&"cal".into());
        let hidden = MonthSnapshot::load(&state, date).unwrap();
        assert!(hidden.days.iter().all(|day| day.events.is_empty()));
    }
}
