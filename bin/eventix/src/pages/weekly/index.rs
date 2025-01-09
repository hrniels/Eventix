use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use chrono::{Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use ical::objects::{CalCompType, CalDate, EventLike};
use serde::Deserialize;
use std::{collections::HashMap, str::FromStr, sync::Arc};

use super::Page;
use crate::locale::{self, DateFlags, Locale, TimeFlags};
use crate::objects::DayOccurrence;
use crate::{error::HTMLError, pages::tasks::Tasks};
use crate::{html::filters, pages::events::Events};

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
    minutes: u32,
    week_start: String,
    week_end: String,
    prev_week: String,
    next_week: String,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    content(
        super::new_page(&state).await,
        locale::default(),
        State(state),
        Query(req),
    )
    .await
}

fn overlaps_of(day_occs: &[&DayOccurrence], occ: &DayOccurrence) -> usize {
    let mut overlaps = vec![];
    for day in day_occs {
        if day.id() == occ.id() {
            continue;
        }
        if let Some(dend) = day.occurrence_end() {
            if occ.overlaps(day.occurrence_start(), dend) {
                overlaps.push(*day);
            }
        }
    }

    if overlaps.is_empty() {
        // it's just us
        1
    } else {
        // otherwise we want to determine the "widest" spot. For example, if there are 3 blocks
        // like that:
        // +-+-+-+
        // | |2|4|
        // | +-+-+
        // |1|
        // | +-+
        // | |3|
        // +-+-+
        // Here block 1 overlaps with 2, 3, and 4, but all not at the same time. For example, 2 and
        // 3 don't overlap with each other. To calculate that we determine the number of overlaps
        // the blocks we overlap have and take the maximum. In the example, 2 overlaps with 4 and 1
        // and, resulting in 3. And 3 overlaps just with 1, resulting in 2. So, we have at most 3
        // overlaps.
        1 + overlaps
            .iter()
            .map(|o| overlaps_of(&overlaps[..], o))
            .max()
            .unwrap()
    }
}

fn determine_slot(slots: &[Vec<&DayOccurrence>], day: &DayOccurrence) -> usize {
    'outer: for slot in 0.. {
        // if the slot is not present yet, we can take it
        if slot >= slots.len() {
            return slot;
        }
        for occ in &slots[slot] {
            if let Some(dend) = day.occurrence_end() {
                if occ.overlaps(day.occurrence_start(), dend) {
                    // if one occurrence in that slot overlaps with us, try the next one
                    continue 'outer;
                }
            }
        }
        // no overlap -> use this slot
        return slot;
    }
    unreachable!();
}

fn get_overlaps(day_occs: &[DayOccurrence]) -> HashMap<u64, (usize, usize)> {
    // first determine the number of overlaps per occurrence
    let mut counts = HashMap::new();
    let all: Vec<_> = day_occs.iter().collect();
    for day in day_occs {
        let count = overlaps_of(&all, day);
        counts.insert(day.id(), count);
    }

    // now sort them by the number of overlaps in descending order. with that, we start on the
    // left with the smallest bar so that all occurrences with less overlaps can be easily placed
    // next to it.
    let mut all_sorted = all.clone();
    all_sorted.sort_by(|a, b| {
        counts
            .get(&b.id())
            .unwrap()
            .cmp(counts.get(&a.id()).unwrap())
    });

    // now walk through the slots from left to right and put occurrences in if there is no overlap
    // yet. for that reason, we keep the occurrences in the slots and test all for an overlap with
    // a potential new occurrence for a slot.
    let mut overlaps = HashMap::new();
    let mut slots = Vec::new();
    for day in all_sorted {
        let slot = determine_slot(&slots, day);
        if slot >= slots.len() {
            slots.push(Vec::new());
        }
        slots[slot].push(day);

        let count = counts.get(&day.id()).unwrap();
        overlaps.insert(day.id(), (*count, slot));
    }
    overlaps
}

pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let timezone = *locale.timezone();

    let date = match req.date {
        Some(date) => NaiveDate::from_str(&date).context(format!("Invalid date: {}", date))?,
        None => Utc::now().with_timezone(&timezone).naive_local().date(),
    };
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

    let (store, disabled) = state.acquire_store_and_disabled().await;

    let ev_occs = store
        .sources()
        .iter()
        .filter(|s| !disabled.contains(s.id()))
        .flat_map(move |s| {
            s.filtered_occurrences_within(mstart, mend, |c| c.ctype() == CalCompType::Event)
        })
        .collect::<Vec<_>>();

    let mut days = Vec::new();
    while date < end {
        let mut day_occs = DayOccurrence::occurrences_on(&ev_occs, date, &timezone);

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

    let events = Events::new(&store, &disabled, &locale);
    let tasks = Tasks::new(&store, &disabled, &locale);

    let now = Utc::now().with_timezone(&timezone);

    let html = WeeklyTemplate {
        page,
        locale: locale.clone(),
        week_start: locale.fmt_weekdate(&week_start, DateFlags::None),
        week_end: locale.fmt_weekdate(&week_end.pred_opt().unwrap(), DateFlags::None),
        prev_week: prev_week.format("%Y-%m-%d").to_string(),
        next_week: next_week.format("%Y-%m-%d").to_string(),
        today: now.date_naive(),
        minutes: now.time().hour() * 60 + now.time().minute(),
        days,
        events,
        tasks,
    }
    .render()
    .context("weekly template")?;

    Ok(Html(html))
}
