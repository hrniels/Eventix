use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use chrono::{Datelike, Duration, NaiveDate, TimeZone, Utc};
use ical::{
    col::CalStore,
    objects::{CalCompType, CalDate, EventLike},
    util,
};
use serde::Deserialize;
use std::{str::FromStr, sync::Arc};

use super::Page;
use crate::locale::{self, Locale};
use crate::objects::DayOccurrence;
use crate::{error::HTMLError, pages::tasks::Tasks};
use crate::{html::filters, pages::events::Events};

struct Day<'a> {
    date: Option<NaiveDate>,
    show_month: bool,
    cur_month: bool,
    occurrences: Vec<DayOccurrence<'a>>,
}

#[derive(Default, Debug, Deserialize)]
pub struct Request {
    month: Option<String>,
}

#[derive(Template)]
#[template(path = "pages/monthly.htm")]
struct MonthlyTemplate<'a> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    weekdays: Vec<&'a str>,
    days: Vec<Day<'a>>,
    today: NaiveDate,
    month: String,
    prev_month: String,
    next_month: String,
    store: &'a CalStore,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    content(
        super::new_page(),
        locale::default(),
        State(state),
        Query(req),
    )
    .await
}

pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
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

    let store = state.store().lock().await;

    let ev_occs = store
        .filtered_occurrences_within(mstart, mend, |c| c.ctype() == CalCompType::Event)
        .collect::<Vec<_>>();

    let mut days = Vec::new();
    while date < end {
        let day_occs = DayOccurrence::occurrences_on(&ev_occs, date, &timezone);
        days.push(Day {
            date: Some(date),
            show_month: date.day() == 1
                || date.day() == util::month_days(date.year(), date.month()),
            cur_month: date >= month_start && date < month_end,
            occurrences: day_occs,
        });

        date += Duration::days(1);
    }

    let events = Events::new(&store, &locale);
    let tasks = Tasks::new(&store, &locale);

    let html = MonthlyTemplate {
        page,
        locale,
        weekdays,
        month: month_start.format("%B %Y").to_string(),
        prev_month: format!("{}-{}", pyear, pmonth),
        next_month: format!("{}-{}", nyear, nmonth),
        today: Utc::now().with_timezone(&timezone).date_naive(),
        store: &store,
        days,
        events,
        tasks,
    }
    .render()
    .context("overview template")?;

    Ok(Html(html))
}
