use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use chrono::{Datelike, Duration, NaiveDate, TimeZone, Utc};
use eventix_ical::objects::{CalCompType, CalDate, CalPartStat, EventLike};
use eventix_locale::{DateFlags, Locale, TimeFlags};
use eventix_state::EventixState;
use serde::Deserialize;
use std::{collections::HashMap, fmt, sync::Arc};

use crate::html::filters;
use crate::objects::{DayOccurrence, OccurrenceOverlap};
use crate::pages::{Page, error::HTMLError, events::Events, tasks::Tasks};
use crate::util::parse_human_date;

struct Day<'a> {
    date: NaiveDate,
    allday: Vec<DayOccurrence<'a>>,
    occurrences: Vec<DayOccurrence<'a>>,
}

#[derive(Default, Debug, Deserialize)]
pub struct Request {
    date: Option<String>,
}

#[derive(Template)]
#[template(path = "pages/weekly.htm")]
struct WeeklyTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    days: Vec<Day<'a>>,
    today: NaiveDate,
    week_number: String,
    week_start: String,
    week_end: String,
    prev_week: String,
    next_week: String,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = state.lock().await.settings().locale();
    content(
        super::new_page(&state).await,
        locale,
        State(state),
        Query(req),
    )
    .await
}

#[derive(Debug)]
struct Rows<'d>(Vec<Row<'d>>);

#[derive(Debug)]
struct Row<'d>(Vec<Slot<'d>>);

struct Slot<'d>(Vec<&'d DayOccurrence<'d>>);

impl<'d> Rows<'d> {
    fn get_overlap(&self, occ: &DayOccurrence) -> OccurrenceOverlap {
        for row in &self.0 {
            for (s, slot) in row.0.iter().enumerate() {
                for o in &slot.0 {
                    if o.id() == occ.id() {
                        // determine how many slots right of this one are free for us
                        let mut width = 1;
                        let mut next = s + 1;
                        while next < row.0.len() && !row.0[next].overlaps_with(occ) {
                            width += 1;
                            next += 1;
                        }
                        return OccurrenceOverlap::new(row.0.len(), s, width);
                    }
                }
            }
        }
        unreachable!();
    }

    fn insert(&mut self, occ: &'d DayOccurrence) {
        for row in &mut self.0 {
            // if there is any overlap in this row, the occurrence *has* to be put into this row
            if row.overlaps_with(occ) {
                for slot in &mut row.0 {
                    // use the first slot it does not overlap with
                    if !slot.overlaps_with(occ) {
                        slot.0.push(occ);
                        return;
                    }
                }
                // ok, all non-overlapping slots - add a new one
                row.0.push(Slot(vec![occ]));
                return;
            }
        }

        // no overlapping row - add a new one
        self.0.push(Row(vec![Slot(vec![occ])]));
    }
}

impl<'d> Row<'d> {
    fn overlaps_with(&self, occ: &DayOccurrence) -> bool {
        self.0.iter().any(|s| s.overlaps_with(occ))
    }
}

impl<'d> Slot<'d> {
    fn overlaps_with(&self, occ: &DayOccurrence) -> bool {
        self.0.iter().any(|o| {
            let ostart = o.occurrence_start().unwrap();
            let oend = o.occurrence_end().unwrap();
            occ.overlaps(ostart, oend)
        })
    }
}

impl<'d> fmt::Debug for Slot<'d> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let summaries: Vec<String> = self
            .0
            .iter()
            .filter_map(|occ| occ.summary().map(|s| s.clone()))
            .collect();
        f.debug_tuple("Slot").field(&summaries).finish()
    }
}

fn get_overlaps(day_occs: &[DayOccurrence]) -> HashMap<u64, OccurrenceOverlap> {
    // first insert all of them into our rows datastructure that puts occurrences into the same row
    // if they overlap or in separate rows otherwise.
    let mut rows = Rows(vec![]);
    for occ in day_occs {
        rows.insert(occ);
    }

    // now determine the overlap for every occurrence
    let mut overlaps = HashMap::new();
    for occ in day_occs {
        overlaps.insert(occ.id(), rows.get_overlap(occ));
    }
    overlaps
}

pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let timezone = *locale.timezone();

    let date = parse_human_date(req.date, &timezone)?;
    let prev_week = date - Duration::days(7);
    let next_week = date + Duration::days(7);

    let week_off = date.weekday().num_days_from_monday();
    let week_start = date - Duration::days(week_off.into());
    let week_end = week_start + Duration::days(7);
    let mut date = week_start;
    let end = week_end;

    let mstart = timezone
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .unwrap();
    let mend = timezone
        .from_local_datetime(&end.pred_opt().unwrap().and_hms_opt(23, 59, 59).unwrap())
        .unwrap();

    let state = state.lock().await;

    let ev_occs = state
        .store()
        .directories()
        .iter()
        .filter(|s| !state.misc().calendar_disabled(s.id()))
        .flat_map(move |s| s.occurrences_between(mstart, mend, |c| c.ctype() == CalCompType::Event))
        .filter(|o| !o.is_excluded())
        .collect::<Vec<_>>();

    let settings = state.settings();
    let pers_alarms = state.personal_alarms();

    let mut days = Vec::new();
    while date < end {
        let mut day_occs =
            DayOccurrence::occurrences_on(&ev_occs, settings, pers_alarms, date, &timezone);

        let mut allday = vec![];
        let mut i = 0;
        while i < day_occs.len() {
            if day_occs[i].is_all_day() || day_occs[i].is_all_day_on(date) {
                allday.push(day_occs.remove(i));
            } else {
                i += 1;
            }
        }

        let counts = get_overlaps(&day_occs);
        for day in &mut day_occs {
            day.set_overlap(*counts.get(&day.id()).unwrap());
        }

        days.push(Day {
            date,
            allday,
            occurrences: day_occs,
        });

        date += Duration::days(1);
    }

    let events = Events::new(&state, &locale);
    let tasks = Tasks::new(&state, &locale);

    let now = Utc::now().with_timezone(&timezone);

    let html = WeeklyTemplate {
        page,
        locale: locale.clone(),
        week_number: week_start.format("%V").to_string(),
        week_start: locale.fmt_weekdate(&week_start, DateFlags::NoToday),
        week_end: locale.fmt_weekdate(&week_end.pred_opt().unwrap(), DateFlags::NoToday),
        prev_week: prev_week.format("%Y-%m-%d").to_string(),
        next_week: next_week.format("%Y-%m-%d").to_string(),
        today: now.date_naive(),
        days,
        events,
        tasks,
    }
    .render()
    .context("weekly template")?;

    Ok(Html(html))
}
