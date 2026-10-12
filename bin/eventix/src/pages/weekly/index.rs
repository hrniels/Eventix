// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use chrono::{Datelike, Duration, Local, MappedLocalTime, NaiveDate, TimeZone};
use eventix_ical::objects::{CalCompType, EventLike};
use eventix_locale::{DateFlags, Locale};
use eventix_state::EventixState;
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

use crate::comps::occurrence::{OccurrenceMode, OccurrenceTemplate};
use crate::html::filters;
use crate::objects::{DayOccurrence, OccurrenceOverlap};
use crate::pages::Page;
use crate::pages::error::HTMLError;
use crate::util::parse_human_date;

struct Day {
    date: NaiveDate,
    allday: Vec<String>,
    allday_ids: Vec<String>,
    occurrences: Vec<String>,
    occ_ids: Vec<String>,
    gaps: Vec<DSTZone>,
    folds: Vec<DSTZone>,
}

#[derive(serde::Serialize)]
struct DSTZone {
    top: u32,
    height: u32,
}

#[derive(Default, Debug, Deserialize)]
pub struct Request {
    date: Option<String>,
}

/// Fragment-only template for the weekly grid, rendered by the AJAX content endpoint.
#[derive(Template)]
#[template(path = "pages/weekly.htm")]
struct WeeklyTemplate {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    days: Vec<Day>,
    today: NaiveDate,
    week_number: String,
    week_start: String,
    week_end: String,
    prev_week: String,
    next_week: String,
}

/// Builds a map from occurrence ID to its overlap descriptor for all occurrences in a day.
///
/// Occurrences are arranged into rows such that overlapping occurrences share a row. Within each
/// row, each occurrence is placed in the first slot that does not overlap with it. The resulting
/// map encodes, for every occurrence, the total number of columns in its row, its column index,
/// and the number of adjacent free columns to the right.
fn get_overlaps(day_occs: &[DayOccurrence]) -> HashMap<u64, OccurrenceOverlap> {
    let intervals: Vec<_> = day_occs
        .iter()
        .map(|occ| {
            let start = occ.occurrence_start().unwrap().timestamp();
            let end = occ
                .occurrence_end()
                .map(|end| end.timestamp())
                .unwrap_or(start);
            (start, end)
        })
        .collect();
    day_occs
        .iter()
        .zip(eventix_view::layout::columns(&intervals))
        .map(|(occ, columns)| {
            (
                occ.id(),
                OccurrenceOverlap::new(columns.count, columns.offset, columns.span),
            )
        })
        .collect()
}

/// Renders only the weekly grid fragment for the given week. Used by the AJAX content endpoint.
pub async fn content(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(&state).await;
    let locale = state.lock().await.locale();
    let timezone = *locale.timezone();
    let now = Local::now().with_timezone(&timezone);

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
        let mut gaps = Vec::new();
        let mut folds = Vec::new();
        for hour in 0..24 {
            for minute in [0, 30] {
                let naive = date.and_hms_opt(hour, minute, 0).unwrap();
                match timezone.from_local_datetime(&naive) {
                    MappedLocalTime::None => {
                        gaps.push(DSTZone {
                            top: hour * 60 + minute,
                            height: 30,
                        });
                    }
                    MappedLocalTime::Ambiguous(_, _) => {
                        folds.push(DSTZone {
                            top: hour * 60 + minute,
                            height: 30,
                        });
                    }
                    _ => {}
                }
            }
        }

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

        let mut allday_rendered = Vec::new();
        let mut allday_ids = Vec::new();
        for occ in allday {
            allday_ids.push(occ.id().to_string());
            allday_rendered.push(
                OccurrenceTemplate::new(locale.clone(), &occ, OccurrenceMode::Block, date, &now)
                    .render()
                    .context("allday occurrence template")?,
            );
        }

        let mut occ_rendered = Vec::new();
        let mut occ_ids = Vec::new();
        for occ in day_occs {
            occ_ids.push(occ.id().to_string());
            occ_rendered.push(
                OccurrenceTemplate::new(locale.clone(), &occ, OccurrenceMode::Timed, date, &now)
                    .render()
                    .context("timed occurrence template")?,
            );
        }

        days.push(Day {
            date,
            allday: allday_rendered,
            allday_ids,
            occurrences: occ_rendered,
            occ_ids,
            gaps,
            folds,
        });

        date += Duration::days(1);
    }

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
    }
    .render()
    .context("weekly content template")?;

    Ok(Html(html))
}
