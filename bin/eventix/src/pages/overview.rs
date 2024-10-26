use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use ical::{
    col::{CalStore, Occurrence},
    objects::{CalComponent, CalTodoStatus, EventLike},
    util,
};
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::{
    cmp::Ordering,
    ops::Deref,
    str::FromStr,
    sync::{Arc, Mutex},
};

use super::Page;
use crate::error::HTMLError;
use crate::html::filters;
use crate::locale::{self, Locale};

struct DayOccurrence<'a> {
    id: u64,
    inner: &'a Occurrence<'a>,
}

impl<'a> DayOccurrence<'a> {
    fn new(inner: &'a Occurrence<'a>) -> Self {
        static NEXT_ID: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(0));
        let mut next = NEXT_ID.lock().unwrap();
        let id = *next + 1;
        *next += 1;
        Self { id, inner }
    }

    fn js_uid(&self) -> String {
        self.inner
            .uid()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect()
    }

    fn status_class(&self) -> Option<String> {
        self.inner.event_status().map(|st| format!("{:?}", st))
    }
}

impl<'a> Deref for DayOccurrence<'a> {
    type Target = Occurrence<'a>;
    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

struct Day<'a> {
    date: Option<NaiveDate>,
    show_month: bool,
    cur_month: bool,
    occurrences: Vec<DayOccurrence<'a>>,
}

#[derive(Debug, Deserialize)]
struct Request {
    month: Option<String>,
}

#[derive(Template)]
#[template(path = "pages/overview.htm")]
struct OverviewTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    weekdays: Vec<&'a str>,
    days: Vec<Day<'a>>,
    today: NaiveDate,
    month: String,
    prev_month: String,
    next_month: String,
    store: &'a CalStore,
    next_events: Vec<Day<'a>>,
    next_tasks: Vec<Day<'a>>,
}

fn get_overlapping_occurrences<'a>(
    ev_occs: &'a [Occurrence<'a>],
    date: NaiveDate,
    timezone: &Tz,
) -> Vec<DayOccurrence<'a>> {
    let day_start = timezone
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .unwrap();
    let day_end = timezone
        .from_local_datetime(&date.and_hms_opt(23, 59, 59).unwrap())
        .unwrap();

    let mut day_occs = ev_occs
        .iter()
        .filter(|o| o.overlaps(day_start, day_end))
        .map(DayOccurrence::new)
        .collect::<Vec<_>>();
    day_occs.sort_by(|a, b| match (a.is_all_day(), b.is_all_day()) {
        (true, true) | (false, false) => a.occurrence_start().cmp(&b.occurrence_start()),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
    });
    day_occs
}

fn get_due_occurrences<'a>(
    ev_occs: &'a [Occurrence<'a>],
    date: NaiveDate,
) -> Vec<DayOccurrence<'a>> {
    let mut day_occs = ev_occs
        .iter()
        .filter(|o| match o.end_or_due() {
            Some(end) => end.as_naive_date() == date,
            None => false,
        })
        .map(DayOccurrence::new)
        .collect::<Vec<_>>();
    day_occs.sort_by(|a, b| match (a.is_all_day(), b.is_all_day()) {
        (true, true) | (false, false) => a.end_or_due().cmp(&b.end_or_due()),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
    });
    day_occs
}

async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(path().to_string());
    let locale = locale::default();
    let timezone = *locale.timezone();

    let weekdays = vec![
        locale.translate("Monday"),
        locale.translate("Tuesday"),
        locale.translate("Wednesday"),
        locale.translate("Thursday"),
        locale.translate("Friday"),
        locale.translate("Saturday"),
        locale.translate("Sunday"),
    ];

    let date = match req.month {
        Some(month) => NaiveDate::from_str(&format!("{}-01", month))
            .context(format!("Invalid month: {}", month))?,
        None => Utc::now().with_timezone(&timezone).naive_local().date(),
    };
    let (pyear, pmonth) = util::prev_month(date.year(), date.month());
    let (nyear, nmonth) = util::next_month(date.year(), date.month());

    let num_days = util::month_days(date.year(), date.month());
    let month_start = NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap();
    let month_end = month_start + Duration::days(num_days as i64);
    let start_off = month_start.weekday().num_days_from_monday();
    let end_off = 7 - month_end.weekday().num_days_from_monday();

    let mut date = month_start - Duration::days(start_off as i64);
    let end = month_start + Duration::days((num_days + end_off) as i64);
    let mstart = timezone
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .unwrap();
    let mend = timezone
        .from_local_datetime(&end.pred_opt().unwrap().and_hms_opt(23, 59, 59).unwrap())
        .unwrap();

    let ev_occs = state
        .store()
        .filtered_occurrences_within(mstart, mend, |c| c.is_event())
        .collect::<Vec<_>>();

    let mut days = Vec::new();
    while date < end {
        let day_occs = get_overlapping_occurrences(&ev_occs, date, &timezone);
        days.push(Day {
            date: Some(date),
            show_month: date.day() == 1
                || date.day() == util::month_days(date.year(), date.month()),
            cur_month: date >= month_start && date < month_end,
            occurrences: day_occs,
        });

        date += Duration::days(1);
    }

    let now = Local::now();
    let start = now.with_timezone(locale.timezone());
    let end = start + Duration::days(7);

    let next_ev_occs = state
        .store()
        .filtered_occurrences_within(start, end, |c| c.is_event())
        .collect::<Vec<_>>();

    let mut next_events = Vec::new();
    let mut cur_date = start.date_naive();
    let end_date = end.date_naive();
    while cur_date < end_date {
        let day_occs = get_overlapping_occurrences(&next_ev_occs, cur_date, &timezone);
        if !day_occs.is_empty() {
            next_events.push(Day {
                date: Some(cur_date),
                show_month: false,
                cur_month: false,
                occurrences: day_occs,
            });
        }

        cur_date += Duration::days(1);
    }

    let mut next_td_occs = state
        .store()
        .filtered_occurrences_within(start, end, |c| c.is_todo())
        .collect::<Vec<_>>();

    let overdue_tds = state
        .store()
        .filtered_occurrences_within(
            DateTime::<Tz>::MIN_UTC.with_timezone(&timezone),
            start,
            |c| match c {
                CalComponent::Todo(td) if td.due().is_some() => {
                    td.status().unwrap_or(CalTodoStatus::NeedsAction) != CalTodoStatus::Completed
                }
                _ => false,
            },
        )
        .filter(|o| {
            // so far, we got all todos that overlap with this period of time. but we are only
            // interested in the ones that are due before the start.
            o.end_or_due()
                .map(|e| e.as_end_with_tz(&timezone))
                .unwrap_or(start)
                < start
        });
    next_td_occs.extend(overdue_tds);

    let mut next_tasks = Vec::new();
    let mut cur_date = next_td_occs
        .iter()
        .map(|o| {
            o.end_or_due()
                .map(|e| e.as_end_with_tz(&timezone))
                .unwrap_or(start)
        })
        .min()
        .unwrap_or(start)
        .date_naive();
    let end_date = end.date_naive();
    while cur_date < end_date {
        let day_occs = get_due_occurrences(&next_td_occs, cur_date);
        if !day_occs.is_empty() {
            next_tasks.push(Day {
                date: Some(cur_date),
                show_month: false,
                cur_month: false,
                occurrences: day_occs,
            });
        }

        cur_date += Duration::days(1);
    }

    let mut unplanned_occs = next_td_occs
        .iter()
        .filter(|o| {
            o.end_or_due().is_none()
                && o.todo_status().unwrap_or(CalTodoStatus::NeedsAction) != CalTodoStatus::Completed
        })
        .map(DayOccurrence::new)
        .collect::<Vec<_>>();
    if !unplanned_occs.is_empty() {
        unplanned_occs.sort_by(|a, b| a.created().cmp(b.created()));
        next_tasks.push(Day {
            date: None,
            show_month: false,
            cur_month: false,
            occurrences: unplanned_occs,
        });
    }

    let html = OverviewTemplate {
        page,
        locale,
        weekdays,
        month: month_start.format("%B %Y").to_string(),
        prev_month: format!("{}-{}", pyear, pmonth),
        next_month: format!("{}-{}", nyear, nmonth),
        today: Utc::now().with_timezone(&timezone).date_naive(),
        store: state.store(),
        days,
        next_events,
        next_tasks,
    }
    .render()
    .context("overview template")?;

    Ok(Html(html))
}

pub fn path() -> &'static str {
    "/"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new().route("/", get(handler)).with_state(state)
}
