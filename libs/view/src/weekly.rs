// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Toolkit-independent projection and placement of a week's events.

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

use crate::{
    Event, OccurrenceId, details,
    layout::{self, Columns},
};

/// Stable identity of a timed day segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedSegmentId {
    pub occurrence: OccurrenceId,
    pub date: NaiveDate,
}

/// One timed day segment and its wall-clock placement.
#[derive(Clone, Debug)]
pub struct TimedEvent {
    pub event: Event,
    pub date: NaiveDate,
    pub minute_start: u32,
    pub minute_end: u32,
    pub columns: Columns,
}

impl TimedEvent {
    /// Returns the identity of this displayed day segment.
    pub fn id(&self) -> TimedSegmentId {
        TimedSegmentId {
            occurrence: self.event.id.clone(),
            date: self.date,
        }
    }
}

/// A contiguous strip span clipped to the visible week, with an exclusive end day.
///
/// Contains true all-day events as well as the interior strip days of multi-day timed events.
/// Day indices are relative to the week's Monday; continuation flags describe whether the
/// original occurrence extends before or after this visible span.
#[derive(Clone, Debug)]
pub struct AllDaySpan {
    pub event: Event,
    /// Inclusive day-column index in `0..7`.
    pub start_day: usize,
    /// Exclusive day-column index in `1..=7`.
    pub end_day: usize,
    /// Zero-based row shared only with non-overlapping spans.
    pub row: usize,
    pub continues_before: bool,
    pub continues_after: bool,
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
    pub timed: Vec<TimedEvent>,
    pub dst: Vec<DstRange>,
}

/// An immutable week snapshot that can be sent from a worker to a UI thread.
pub struct WeekSnapshot {
    pub start: NaiveDate,
    pub days: Vec<Day>,
    pub all_day: Vec<AllDaySpan>,
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
    /// Timed events are split into day segments; contiguous strip days of an occurrence are
    /// combined into one span and packed into non-overlapping rows.
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
        let details: Vec<_> = occurrences
            .iter()
            .map(|occ| details::Details::load(state, occ).map(Arc::new))
            .collect();
        let mut days = Vec::new();
        let mut strip_days = Vec::new();
        for offset in 0..7 {
            let date = start + Duration::days(offset);
            let mut day = Day {
                date,
                timed: Vec::new(),
                dst: dst_ranges(date, tz),
            };
            let mut strip = Vec::new();
            for (occ, details) in occurrences.iter().zip(&details) {
                let Some(details) = details else { continue };
                let Some(segment) = project_event(state, occ, date, details.clone()) else {
                    continue;
                };
                if segment.event.all_day || occ.is_all_day_on(date) {
                    strip.push(segment.event);
                } else {
                    day.timed.push(segment);
                }
            }
            day.timed.sort_by(|a, b| event_order(&a.event, &b.event));
            strip.sort_by(event_order);
            strip_days.push(strip);
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
            all_day: strip_spans(start, &strip_days),
            locale: state.locale(),
        })
    }
}

fn project_event(
    state: &State,
    occ: &Occurrence<'_>,
    date: NaiveDate,
    details: Arc<details::Details>,
) -> Option<TimedEvent> {
    let tz = state.timezone();
    let start = occ.occurrence_start()?.with_timezone(tz);
    let end = occ.occurrence_end().unwrap_or(start).with_timezone(tz);
    let (minute_start, minute_end) = day_minutes(start, end, date)?;
    let (_, settings) = state.settings().calendar(occ.directory())?;
    let event = Event {
        id: OccurrenceId {
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
        },
        summary: occ.summary().cloned().unwrap_or_default(),
        calendar: settings.name().clone(),
        foreground: settings.fgcolor().clone(),
        background: settings.bgcolor().clone(),
        details,
        start,
        end,
        all_day: occ.is_all_day(),
        cancelled: occ.is_cancelled(),
        recurrent: occ.is_recurrent(),
        exception: occ.is_overwritten(),
        alarm: state.personal_alarms().has_alarms(occ, settings.alarms()),
    };
    Some(TimedEvent {
        event,
        date,
        minute_start,
        minute_end,
        columns: Columns::default(),
    })
}

fn event_order(a: &Event, b: &Event) -> std::cmp::Ordering {
    (a.start, &a.id.calendar, &a.summary, &a.id).cmp(&(b.start, &b.id.calendar, &b.summary, &b.id))
}

/// Combines only adjacent strip segments, retaining the existing timed/strip classification.
fn strip_spans(start: NaiveDate, days: &[Vec<Event>]) -> Vec<AllDaySpan> {
    let mut spans: Vec<AllDaySpan> = Vec::new();
    for (day, events) in days.iter().enumerate() {
        for event in events {
            if let Some(span) = spans
                .iter_mut()
                .find(|span| span.event.id == event.id && span.end_day == day)
            {
                span.end_day = day + 1;
            } else {
                spans.push(AllDaySpan {
                    event: event.clone(),
                    start_day: day,
                    end_day: day + 1,
                    row: 0,
                    continues_before: false,
                    continues_after: false,
                });
            }
        }
    }
    spans.sort_by(|a, b| event_order(&a.event, &b.event));
    let intervals: Vec<_> = spans
        .iter()
        .map(|span| (span.start_day, span.end_day))
        .collect();
    for (span, row) in spans.iter_mut().zip(layout::rows(&intervals)) {
        span.row = row;
        // Continuation indicators describe the original occurrence, including timed boundaries.
        let tz = span.event.start.timezone();
        let from = start + Duration::days(span.start_day as i64);
        let to = start + Duration::days(span.end_day as i64);
        span.continues_before =
            span.event.start < util::resolve_local_time(&tz, from.and_hms_opt(0, 0, 0).unwrap());
        span.continues_after =
            span.event.end > util::resolve_local_time(&tz, to.and_hms_opt(0, 0, 0).unwrap());
    }
    spans
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
    fn strip_spans_clip_pack_and_keep_recurring_instances_distinct() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let mut state = State::new_with_timezone(Arc::new(xdg), Some(Berlin)).unwrap();
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: tmp.path().to_string_lossy().into_owned(),
        });
        let mut settings = CalendarSettings::default();
        settings.set_enabled(true);
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
            "UID:long\nDTSTART;VALUE=DATE:20261003\nDTEND;VALUE=DATE:20261014",
            "UID:series\nDTSTART;VALUE=DATE:20261005\nDTEND;VALUE=DATE:20261007\nRRULE:FREQ=DAILY;COUNT=2",
            "UID:timed\nDTSTART;TZID=Europe/Berlin:20261005T120000\nDTEND;TZID=Europe/Berlin:20261009T120000",
        ];
        for (i, event) in events.iter().enumerate() {
            let calendar = format!(
                "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\n{event}\nEND:VEVENT\nEND:VCALENDAR\n"
            )
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
        let long = week
            .all_day
            .iter()
            .find(|span| span.event.id.uid == "long")
            .unwrap();
        assert_eq!((long.start_day, long.end_day, long.row), (0, 7, 0));
        assert!(long.continues_before && long.continues_after);
        let series: Vec<_> = week
            .all_day
            .iter()
            .filter(|span| span.event.id.uid == "series")
            .collect();
        assert_eq!(series.len(), 2);
        assert_ne!(series[0].event.id, series[1].event.id);
        assert_eq!((series[0].start_day, series[0].end_day), (0, 2));
        assert_eq!((series[1].start_day, series[1].end_day), (1, 3));
        assert_ne!(series[0].row, series[1].row);
        let timed = week
            .all_day
            .iter()
            .find(|span| span.event.id.uid == "timed")
            .unwrap();
        assert!(!timed.event.all_day);
        assert_eq!((timed.start_day, timed.end_day), (1, 4));
        assert!(timed.continues_before && timed.continues_after);
        assert_eq!(week.days[0].timed.len(), 1);
        assert_eq!(week.days[4].timed.len(), 1);
        for (i, a) in week.all_day.iter().enumerate() {
            for b in &week.all_day[i + 1..] {
                assert!(a.row != b.row || a.end_day <= b.start_day || b.end_day <= a.start_day);
            }
        }
        let refreshed = WeekSnapshot::load(&state, date).unwrap();
        assert_eq!(
            week.all_day
                .iter()
                .map(|span| (&span.event.id, span.row))
                .collect::<Vec<_>>(),
            refreshed
                .all_day
                .iter()
                .map(|span| (&span.event.id, span.row))
                .collect::<Vec<_>>()
        );
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
                "LOCATION:Original room\nDESCRIPTION:Original description\nSTATUS:CONFIRMED\n",
                "EXDATE;TZID=Europe/Berlin:20261006T090000\nSUMMARY:Series\nEND:VEVENT\n",
                "BEGIN:VEVENT\nUID:series\nRECURRENCE-ID;TZID=Europe/Berlin:20261007T090000\n",
                "DTSTART;TZID=Europe/Berlin:20261007T110000\n",
                "DTEND;TZID=Europe/Berlin:20261007T120000\nSUMMARY:Moved\n",
                "LOCATION:New room\nDESCRIPTION:Updated description\nSTATUS:TENTATIVE\nEND:VEVENT",
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
        assert_eq!(week.all_day.len(), 1);
        assert_eq!((week.all_day[0].start_day, week.all_day[0].end_day), (0, 2));
        assert!(!week.all_day[0].continues_before);
        assert!(!week.all_day[0].continues_after);
        assert_eq!(
            week.days.iter().map(|d| d.timed.len()).collect::<Vec<_>>(),
            [2, 0, 1, 0, 0, 0, 0]
        );
        assert_eq!(week.days[0].timed[1].minute_end, 1440);
        assert_eq!(
            week.all_day[0].event.details.date,
            "Oct 05, 2026 ‒ Oct 06, 2026"
        );
        let segment = &week.days[2].timed[0];
        let moved = &segment.event;
        assert!(moved.exception);
        assert_eq!(moved.summary, "Moved");
        assert_eq!(segment.minute_start, 660);
        assert!(moved.details.date.contains("11:00"));
        assert_eq!(moved.details.location, "New room");
        assert_eq!(moved.details.description, "Updated description");
        assert_eq!(moved.details.status.as_deref(), Some("TENTATIVE"));
        assert_eq!(moved.background, "#123456");
        assert!(!moved.id.recurrence.is_empty());
        let refreshed = WeekSnapshot::load(&state, date).unwrap();
        assert_eq!(segment.id(), refreshed.days[2].timed[0].id());
        state.misc_mut().toggle_calendar(&"cal".into());
        let hidden = WeekSnapshot::load(&state, date).unwrap();
        assert!(hidden.days.iter().all(|d| d.timed.is_empty()));
        assert!(hidden.all_day.is_empty());
    }
}
