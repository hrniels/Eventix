// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Directional search of visible event occurrences.

use chrono::{DateTime, Duration};
use chrono_tz::Tz;
use eventix_ical::{
    col::{CalFile, Occurrence},
    objects::{CalCompType, CalComponent, CompDateType, EventLike},
};
use eventix_state::State;

use super::{OccurrenceId, keywords::KeywordExpression, list_occurrences::limit};

/// A stable chronological position, including ties between simultaneous occurrences.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Cursor {
    pub start: DateTime<Tz>,
    pub id: OccurrenceId,
}

#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub generation: u64,
    pub keywords: String,
    pub now: DateTime<Tz>,
    pub cursor: Option<Cursor>,
    pub forward: bool,
}

impl Cursor {
    fn from_occurrence(occ: &Occurrence<'_>) -> Option<Self> {
        Some(Self {
            start: occ.occurrence_start()?,
            id: OccurrenceId {
                calendar: occ.directory().to_string(),
                uid: occ.uid().clone(),
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
        })
    }
}

impl Request {
    fn candidate(&self, keywords: &KeywordExpression, occ: &Occurrence<'_>) -> Option<Cursor> {
        if occ.is_excluded() || !keywords.matches_event(occ) {
            return None;
        }

        let cursor = Cursor::from_occurrence(occ)?;
        let eligible = self.cursor.as_ref().map_or_else(
            || {
                if self.forward {
                    cursor.start >= self.now
                } else {
                    cursor.start <= self.now
                }
            },
            |previous| {
                if self.forward {
                    cursor > *previous
                } else {
                    cursor < *previous
                }
            },
        );
        eligible.then_some(cursor)
    }

    fn nearer(&self, a: Cursor, b: Cursor) -> Cursor {
        if self.forward { a.min(b) } else { a.max(b) }
    }
}

/// Finds the nearest matching occurrence in the requested direction without a time horizon.
pub(crate) fn search(state: &State, request: &Request) -> Option<Cursor> {
    let keywords = KeywordExpression::parse(&request.keywords);
    state
        .store()
        .directories()
        .iter()
        .filter(|dir| !state.misc().calendar_disabled(dir.id()))
        .flat_map(|dir| dir.files())
        .filter_map(|file| nearest_in_file(file, *state.timezone(), request, &keywords))
        .reduce(|a, b| request.nearer(a, b))
}

fn nearest_in_file(
    file: &CalFile,
    tz: Tz,
    request: &Request,
    keywords: &KeywordExpression,
) -> Option<Cursor> {
    let base = file
        .components()
        .iter()
        .find(|c| c.rid().is_none() && c.ctype() == CalCompType::Event)?;
    if !keywords.matches_event(base) {
        // A nonmatching infinite series must not be expanded looking for a matching
        // exception. Its finite overrides can be resolved directly against the base.
        return file
            .components()
            .iter()
            .filter(|c| c.rid().is_some())
            .filter_map(|overwritten| override_occurrence(file, base, overwritten, tz))
            .filter_map(|occ| request.candidate(keywords, &occ))
            .reduce(|a, b| request.nearer(a, b));
    }

    let boundary = request.cursor.as_ref().map_or(request.now, |c| c.start);
    let (start, end) = if request.forward {
        (boundary, limit(tz, true))
    } else {
        (
            limit(tz, false),
            boundary
                .checked_add_signed(Duration::seconds(1))
                .unwrap_or_else(|| limit(tz, true)),
        )
    };
    // Matching series can stop at the first eligible forward start-time group.
    // Backward scans retain only one result, not the series' history.
    let mut best: Option<Cursor> = None;
    for occ in file.occurrences_between(start, end, |_| true) {
        if request.forward
            && best
                .as_ref()
                .is_some_and(|best| occ.occurrence_start().is_some_and(|date| date > best.start))
        {
            break;
        }
        if let Some(cursor) = request.candidate(keywords, &occ) {
            best = Some(match best {
                Some(previous) => request.nearer(previous, cursor),
                None => cursor,
            });
        }
    }
    best
}

fn override_occurrence<'a>(
    file: &CalFile,
    base: &'a CalComponent,
    overwritten: &'a CalComponent,
    tz: Tz,
) -> Option<Occurrence<'a>> {
    let rid = overwritten.rid()?;
    let resolver = file.calendar().timezone_resolver();
    let date = overwritten
        .start()
        .unwrap_or(rid)
        .as_start_with_resolver(&tz, resolver);
    let excluded = base.exdates().iter().any(|date| {
        date.as_start_with_resolver(&tz, resolver) == rid.as_start_with_resolver(&tz, resolver)
    });
    let mut occ = Occurrence::new_single_in_tz(
        file.directory().clone(),
        base,
        CompDateType::Start,
        date,
        excluded,
        tz,
    );
    occ.set_overwrite(overwritten, &tz, resolver);
    Some(occ)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::TimeZone;
    use eventix_ical::col::{CalDir, CalFile};
    use eventix_state::{CalendarSettings, CollectionSettings, SyncerType};

    use super::{Request, State, search as find};

    fn state(components: &[&str]) -> (tempfile::TempDir, State) {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let mut state =
            State::new_with_timezone(Arc::new(xdg), Some(chrono_tz::Europe::Berlin)).unwrap();
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: tmp.path().display().to_string(),
        });
        let mut settings = CalendarSettings::default();
        settings.set_enabled(true);
        collection
            .all_calendars_mut()
            .insert("work".into(), settings);
        let id = Arc::new("work".to_string());
        let mut dir = CalDir::new_empty(id.clone(), tmp.path().into(), id.to_string(), false);
        for (index, component) in components.iter().enumerate() {
            let calendar = format!("BEGIN:VCALENDAR\nVERSION:2.0\n{component}\nEND:VCALENDAR\n")
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
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        (tmp, state)
    }

    fn request(keywords: &str, forward: bool) -> Request {
        Request {
            generation: 1,
            keywords: keywords.into(),
            now: chrono_tz::Europe::Berlin
                .with_ymd_and_hms(2026, 10, 25, 12, 0, 0)
                .unwrap(),
            cursor: None,
            forward,
        }
    }

    #[test]
    fn search_orders_simultaneous_matches_and_reverses_without_repeating() {
        let (_tmp, state) = state(&[
            "BEGIN:VEVENT\nUID:a\nDTSTART:20261025T120000Z\nSUMMARY:Alpha\nDESCRIPTION:Beta\nLOCATION:Office\nEND:VEVENT",
            "BEGIN:VEVENT\nUID:b\nDTSTART:20261025T120000Z\nSUMMARY:Alpha\nDESCRIPTION:Beta\nEND:VEVENT",
            "BEGIN:VEVENT\nUID:past\nDTSTART;VALUE=DATE:20261024\nSUMMARY:Alpha\nEND:VEVENT",
            "BEGIN:VTODO\nUID:task\nDTSTART:20261025T110000Z\nSUMMARY:Alpha\nEND:VTODO",
        ]);
        let mut query = request("ALPHA beta OR missing", true);
        let a = find(&state, &query).unwrap();
        assert_eq!(a.id.uid, "a");
        query.cursor = Some(a.clone());
        let b = find(&state, &query).unwrap();
        assert_eq!(b.id.uid, "b");
        query.cursor = Some(b.clone());
        assert!(find(&state, &query).is_none());
        query.forward = false;
        assert_eq!(find(&state, &query), Some(a));
        assert_eq!(
            find(&state, &request("alpha", false)).unwrap().id.uid,
            "past"
        );
        assert_eq!(find(&state, &request("office", true)).unwrap().id.uid, "a");
        assert!(find(&state, &request("unknown", true)).is_none());
    }

    #[test]
    fn search_resolves_excluded_and_moved_recurrences_by_effective_fields() {
        let (_tmp, state) = state(&[
            "BEGIN:VEVENT\nUID:series\nDTSTART;VALUE=DATE:20261024\nRRULE:FREQ=DAILY;COUNT=6\nEXDATE;VALUE=DATE:20261026\nSUMMARY:Base\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:series\nRECURRENCE-ID;VALUE=DATE:20261027\nDTSTART;VALUE=DATE:20261102\nSUMMARY:Special\nEND:VEVENT",
        ]);
        let base = find(&state, &request("base", true)).unwrap();
        assert_eq!(base.start.date_naive().to_string(), "2026-10-28");
        let special = find(&state, &request("special", true)).unwrap();
        assert_eq!(special.start.date_naive().to_string(), "2026-11-02");
        assert_eq!(special.id.recurrence, "D2026-10-27;Exclusive");
        let mut backwards = request("base", false);
        backwards.cursor = Some(special);
        assert_eq!(
            find(&state, &backwards)
                .unwrap()
                .start
                .date_naive()
                .to_string(),
            "2026-10-29"
        );
    }

    #[test]
    fn nonmatching_infinite_series_searches_only_finite_effective_exceptions() {
        let (_tmp, state) = state(&[
            "BEGIN:VEVENT\nUID:series\nDTSTART;VALUE=DATE:20261024\nRRULE:FREQ=DAILY\nSUMMARY:Base\nDESCRIPTION:Inherited\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:series\nRECURRENCE-ID;VALUE=DATE:20261027\nDTSTART;VALUE=DATE:20501102\nSUMMARY:Special\nEND:VEVENT",
        ]);
        let mut query = request("special inherited", true);
        let special = find(&state, &query).unwrap();
        assert_eq!(special.start.date_naive().to_string(), "2050-11-02");
        query.cursor = Some(special);
        assert!(find(&state, &query).is_none());
        query.forward = false;
        assert!(find(&state, &query).is_none());
        assert!(find(&state, &request("absent", true)).is_none());
        assert!(find(&state, &request("absent", false)).is_none());
    }

    #[test]
    fn search_uses_profile_timezone_and_skips_disabled_calendars() {
        let (_tmp, mut state) =
            state(&["BEGIN:VEVENT\nUID:all-day\nDTSTART;VALUE=DATE:20261026\nEND:VEVENT"]);
        let mut query = request("", true);
        query.now = chrono_tz::Europe::Berlin
            .with_ymd_and_hms(2026, 10, 26, 0, 0, 0)
            .unwrap();
        assert_eq!(find(&state, &query).unwrap().start, query.now);
        state.misc_mut().toggle_calendar(&"work".into());
        assert!(find(&state, &query).is_none());
    }

    #[test]
    fn search_reports_exhaustion_only_after_the_boundary_match() {
        let (_tmp, state) = state(&[
            "BEGIN:VEVENT\nUID:a\nDTSTART;VALUE=DATE:20261026\nSUMMARY:Meeting\nEND:VEVENT",
            "BEGIN:VEVENT\nUID:b\nDTSTART;VALUE=DATE:20261027\nSUMMARY:Meeting\nEND:VEVENT",
        ]);
        let mut query = request("meeting", true);
        let first = find(&state, &query).unwrap();
        assert_eq!(first.id.uid, "a");
        query.cursor = Some(first.clone());
        let last = find(&state, &query).unwrap();
        assert_eq!(last.id.uid, "b");
        query.cursor = Some(last);
        assert!(find(&state, &query).is_none());
        query.forward = false;
        assert_eq!(find(&state, &query), Some(first.clone()));
        query.cursor = Some(first);
        assert!(find(&state, &query).is_none());
    }
}
