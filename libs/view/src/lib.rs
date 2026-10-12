// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Owned, toolkit-independent data for calendar views.

pub mod layout;

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, MappedLocalTime, NaiveDate, TimeZone, Timelike};
use chrono_tz::Tz;
use eventix_ical::{
    col::Occurrence,
    objects::{CalCompType, EventLike},
    util,
};
use eventix_locale::Locale;
use eventix_state::State;

use layout::Columns;

/// Stable identity of one displayed day segment of an occurrence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventId {
    pub calendar: String,
    pub uid: String,
    pub recurrence: String,
    pub date: NaiveDate,
}

/// An owned event segment, including its original range for the details view.
#[derive(Clone, Debug)]
pub struct Event {
    pub id: EventId,
    pub summary: String,
    pub calendar: String,
    pub foreground: String,
    pub background: String,
    pub location: String,
    pub description: String,
    pub start: DateTime<Tz>,
    pub end: DateTime<Tz>,
    pub all_day: bool,
    pub cancelled: bool,
    pub recurrent: bool,
    pub exception: bool,
    pub alarm: bool,
    pub minute_start: u32,
    pub minute_end: u32,
    pub columns: Columns,
}

/// A wall-clock range affected by a daylight-saving transition.
#[derive(Clone, Debug)]
pub struct DstRange {
    pub start: u32,
    pub end: u32,
    pub fold: bool,
}

/// One day in a week, with timed events in chronological order.
#[derive(Clone, Debug)]
pub struct Day {
    pub date: NaiveDate,
    pub all_day: Vec<Event>,
    pub timed: Vec<Event>,
    pub dst: Vec<DstRange>,
}

/// An immutable week snapshot that can be sent from a worker to a UI thread.
pub struct WeekSnapshot {
    pub start: NaiveDate,
    pub days: Vec<Day>,
    pub locale: Arc<dyn Locale + Send + Sync>,
}

/// Returns the Monday of the week containing `date`, if representable.
pub fn week_start(date: NaiveDate) -> Option<NaiveDate> {
    date.checked_sub_signed(Duration::days(date.weekday().num_days_from_monday().into()))
}

impl WeekSnapshot {
    /// Projects visible, non-excluded events into the week containing `date`.
    ///
    /// Uses the configured display timezone and preserves calendar colors and alarm indicators.
    /// Returns an error if the requested week exceeds the supported date range.
    pub fn load(state: &State, date: NaiveDate) -> Result<Self> {
        let start = week_start(date).context("week start out of range")?;
        let end = start
            .checked_add_signed(Duration::days(7))
            .context("week end out of range")?;
        let tz = *state.timezone();
        let range_start = util::resolve_local_time(&tz, start.and_hms_opt(0, 0, 0).unwrap());
        let range_end = util::resolve_local_time(&tz, end.and_hms_opt(0, 0, 0).unwrap());
        let occurrences: Vec<_> = state
            .store()
            .directories()
            .iter()
            .filter(|dir| !state.misc().calendar_disabled(dir.id()))
            .flat_map(|dir| {
                dir.occurrences_between(range_start, range_end, |c| c.ctype() == CalCompType::Event)
            })
            .filter(|occ| !occ.is_excluded())
            .collect();
        let mut days = Vec::new();
        for offset in 0..7 {
            let date = start + Duration::days(offset);
            let mut day = Day {
                date,
                all_day: Vec::new(),
                timed: Vec::new(),
                dst: dst_ranges(date, tz),
            };
            for occ in &occurrences {
                let Some(event) = project_event(state, occ, date) else {
                    continue;
                };
                if event.all_day || occ.is_all_day_on(date) {
                    day.all_day.push(event);
                } else {
                    day.timed.push(event);
                }
            }
            for events in [&mut day.all_day, &mut day.timed] {
                events.sort_by(|a, b| {
                    (a.start, &a.id.calendar, &a.summary, &a.id.uid).cmp(&(
                        b.start,
                        &b.id.calendar,
                        &b.summary,
                        &b.id.uid,
                    ))
                });
            }
            // Use displayed ranges for collisions, including short events and DST folds.
            let intervals: Vec<_> = day
                .timed
                .iter()
                .map(|e| {
                    (
                        i64::from(e.minute_start.min(1422)),
                        i64::from(e.minute_end.max(e.minute_start + 18)),
                    )
                })
                .collect();
            for (event, columns) in day.timed.iter_mut().zip(layout::columns(&intervals)) {
                event.columns = columns;
            }
            days.push(day);
        }
        Ok(Self {
            start,
            days,
            locale: state.locale(),
        })
    }
}

fn project_event(state: &State, occ: &Occurrence<'_>, date: NaiveDate) -> Option<Event> {
    let tz = state.timezone();
    let start = occ.occurrence_start()?.with_timezone(tz);
    let end = occ.occurrence_end().unwrap_or(start).with_timezone(tz);
    let (minute_start, minute_end) = day_minutes(start, end, date)?;
    let (_, settings) = state.settings().calendar(occ.directory())?;
    Some(Event {
        id: EventId {
            calendar: occ.directory().to_string(),
            uid: occ.uid().to_string(),
            recurrence: occ.rid().map(ToString::to_string).unwrap_or_else(|| {
                if occ.is_recurrent() {
                    occ.occurrence_startdate()
                        .map(|d| d.to_string())
                        .unwrap_or_default()
                } else {
                    String::new()
                }
            }),
            date,
        },
        summary: occ.summary().cloned().unwrap_or_default(),
        calendar: settings.name().clone(),
        foreground: settings.fgcolor().clone(),
        background: settings.bgcolor().clone(),
        location: occ.location().cloned().unwrap_or_default(),
        description: occ.description().cloned().unwrap_or_default(),
        start,
        end,
        all_day: occ.is_all_day(),
        cancelled: occ.is_cancelled(),
        recurrent: occ.is_recurrent(),
        exception: occ.is_overwritten(),
        alarm: state.personal_alarms().has_alarms(occ, settings.alarms()),
        minute_start,
        minute_end,
        columns: Columns::default(),
    })
}

fn day_minutes(start: DateTime<Tz>, end: DateTime<Tz>, date: NaiveDate) -> Option<(u32, u32)> {
    let midnight = util::resolve_local_time(&start.timezone(), date.and_hms_opt(0, 0, 0)?);
    let next = util::resolve_local_time(&start.timezone(), date.succ_opt()?.and_hms_opt(0, 0, 0)?);
    if start >= next || end < midnight || (end == midnight && end != start) {
        return None;
    }
    let from = if start < midnight {
        0
    } else {
        start.hour() * 60 + start.minute()
    };
    let to = if end >= next {
        1440
    } else {
        end.hour() * 60 + end.minute()
    };
    // An event crossing a backward clock change can have an end earlier on the clock.
    Some((from.min(to), from.max(to)))
}

fn dst_ranges(date: NaiveDate, tz: Tz) -> Vec<DstRange> {
    let mut ranges: Vec<DstRange> = Vec::new();
    for minute in 0..1440 {
        let local = date.and_hms_opt(minute / 60, minute % 60, 0).unwrap();
        let fold = match tz.from_local_datetime(&local) {
            MappedLocalTime::Single(_) => continue,
            MappedLocalTime::None => false,
            MappedLocalTime::Ambiguous(_, _) => true,
        };
        if let Some(last) = ranges.last_mut()
            && last.fold == fold
            && last.end == minute
        {
            last.end += 1;
        } else {
            ranges.push(DstRange {
                start: minute,
                end: minute + 1,
                fold,
            });
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use chrono::{NaiveDate, TimeZone};
    use chrono_tz::Europe::Berlin;
    use eventix_ical::col::{CalDir, CalFile};
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};

    use super::{WeekSnapshot, day_minutes, dst_ranges};

    #[test]
    fn midnight_end_and_multiday_clipping() {
        let start = Berlin.with_ymd_and_hms(2026, 10, 5, 23, 0, 0).unwrap();
        let end = Berlin.with_ymd_and_hms(2026, 10, 7, 0, 0, 0).unwrap();
        assert_eq!(
            day_minutes(start, end, start.date_naive()),
            Some((1380, 1440))
        );
        assert_eq!(
            day_minutes(start, end, start.date_naive().succ_opt().unwrap()),
            Some((0, 1440))
        );
        assert_eq!(day_minutes(start, end, end.date_naive()), None);
    }

    #[test]
    fn dst_geometry_uses_wall_clock() {
        for (month, day, fold) in [(3, 29, false), (10, 25, true)] {
            let date = NaiveDate::from_ymd_opt(2026, month, day).unwrap();
            let ranges = dst_ranges(date, Berlin);
            assert_eq!(ranges.len(), 1);
            assert_eq!(
                (ranges[0].start, ranges[0].end, ranges[0].fold),
                (120, 180, fold)
            );
            let start = Berlin.with_ymd_and_hms(2026, month, day, 1, 0, 0).unwrap();
            let end = Berlin.with_ymd_and_hms(2026, month, day, 4, 0, 0).unwrap();
            assert_eq!(day_minutes(start, end, date), Some((60, 240)));
        }
    }

    #[test]
    fn projects_local_calendar_data_with_exclusions_and_stable_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        fs::create_dir_all(tmp.path().join("locale")).unwrap();
        fs::write(tmp.path().join("locale/English.toml"), "[table]\n").unwrap();
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
        // DATE ends are exclusive; timed midnight ends must not leak into the next day.
        let events = [
            concat!(
                "BEGIN:VEVENT\nUID:allday\nDTSTART;VALUE=DATE:20261005\n",
                "DTEND;VALUE=DATE:20261007\nSUMMARY:Two days\nEND:VEVENT",
            ),
            concat!(
                "BEGIN:VEVENT\nUID:midnight\nDTSTART;TZID=Europe/Berlin:20261005T230000\n",
                "DTEND;TZID=Europe/Berlin:20261006T000000\nEND:VEVENT",
            ),
            concat!(
                "BEGIN:VEVENT\nUID:series\nDTSTART;TZID=Europe/Berlin:20261005T090000\n",
                "DTEND;TZID=Europe/Berlin:20261005T100000\nRRULE:FREQ=DAILY;COUNT=3\n",
                "EXDATE;TZID=Europe/Berlin:20261006T090000\nSUMMARY:Series\nEND:VEVENT\n",
                "BEGIN:VEVENT\nUID:series\nRECURRENCE-ID;TZID=Europe/Berlin:20261007T090000\n",
                "DTSTART;TZID=Europe/Berlin:20261007T110000\n",
                "DTEND;TZID=Europe/Berlin:20261007T120000\nSUMMARY:Moved\nEND:VEVENT",
            ),
            concat!(
                "BEGIN:VTODO\nUID:todo\nDTSTART;TZID=Europe/Berlin:20261005T130000\n",
                "DUE;TZID=Europe/Berlin:20261005T140000\nEND:VTODO",
            ),
        ];
        for (i, event) in events.iter().enumerate() {
            let calendar = format!("BEGIN:VCALENDAR\nVERSION:2.0\n{event}\nEND:VCALENDAR\n")
                .parse()
                .unwrap();
            dir.add_file(CalFile::new(
                id.clone(),
                tmp.path().join(format!("{i}.ics")),
                calendar,
            ))
            .unwrap();
        }
        state.store_mut().add(dir);
        let date = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        let week = WeekSnapshot::load(&state, date).unwrap();
        assert_eq!(
            week.days
                .iter()
                .map(|d| d.all_day.len())
                .collect::<Vec<_>>(),
            [1, 1, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            week.days.iter().map(|d| d.timed.len()).collect::<Vec<_>>(),
            [2, 0, 1, 0, 0, 0, 0]
        );
        assert_eq!(week.days[0].timed[1].minute_end, 1440);
        let moved = &week.days[2].timed[0];
        assert!(moved.exception);
        assert_eq!(moved.summary, "Moved");
        assert_eq!(moved.minute_start, 660);
        assert_eq!(moved.background, "#123456");
        assert!(!moved.id.recurrence.is_empty());
        let refreshed = WeekSnapshot::load(&state, date).unwrap();
        assert_eq!(moved.id, refreshed.days[2].timed[0].id);
        state.misc_mut().toggle_calendar(&"cal".into());
        let hidden = WeekSnapshot::load(&state, date).unwrap();
        assert!(
            hidden
                .days
                .iter()
                .all(|d| d.all_day.is_empty() && d.timed.is_empty())
        );
    }
}
