use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use chrono::{Datelike, Duration, NaiveDate, TimeZone, Utc};
use ical::{
    col::{CalStore, Occurrence},
    objects::CalComponent,
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
            .component()
            .uid()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect()
    }

    fn status_class(&self) -> Option<String> {
        match self.inner.component() {
            CalComponent::Event(ev) => ev.status().map(|st| format!("{:?}", st)),
            CalComponent::Todo(td) => td.status().map(|st| format!("{:?}", st)),
        }
    }
}

impl<'a> Deref for DayOccurrence<'a> {
    type Target = Occurrence<'a>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

struct Day<'a> {
    date: NaiveDate,
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
}

async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let page = Page::new(path().to_string());
    let locale = locale::default();
    let timezone = chrono_tz::Europe::Berlin;

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
        .components_within(mstart, mend)
        .filter(|occ| occ.component().is_event())
        .collect::<Vec<_>>();

    let mut days = Vec::new();
    while date < end {
        let day_start = timezone
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .unwrap();
        let day_end = timezone
            .from_local_datetime(&date.and_hms_opt(23, 59, 59).unwrap())
            .unwrap();

        let mut day_occs = ev_occs
            .iter()
            .filter(|o| o.overlaps(day_start, day_end))
            .map(|o| DayOccurrence::new(o))
            .collect::<Vec<_>>();
        day_occs.sort_by(
            |a, b| match (a.component().is_all_day(), b.component().is_all_day()) {
                (true, true) | (false, false) => a.start().cmp(&b.start()),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
            },
        );

        days.push(Day {
            date,
            show_month: date.day() == 1
                || date.day() == util::month_days(date.year(), date.month()),
            cur_month: date >= month_start && date < month_end,
            occurrences: day_occs,
        });

        date += Duration::days(1);
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
