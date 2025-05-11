use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
};
use chrono::{Datelike, Duration, NaiveDate, TimeZone, Utc};
use ical::{
    objects::{CalCompType, CalDate, EventLike},
    util,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::objects::DayOccurrence;
use crate::pages::{Page, error::HTMLError, tasks::Tasks};
use crate::util::parse_human_date;
use crate::{html::filters, pages::events::Events};
use crate::{
    locale::{self, DateFlags, Locale, TimeFlags},
    state::EventixState,
};

struct Day<'a> {
    date: Option<NaiveDate>,
    show_month: bool,
    cur_month: bool,
    occurrences: Vec<DayOccurrence<'a>>,
}

#[derive(Default, Debug, Deserialize)]
pub struct Request {
    date: Option<String>,
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
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<EventixState>,
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

pub async fn content(
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    State(state): State<EventixState>,
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

    let date = parse_human_date(req.date, &timezone)?;
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

    let state = state.lock().await;
    let store = state.store();

    let ev_occs = store
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
        let day_occs =
            DayOccurrence::occurrences_on(&ev_occs, settings, pers_alarms, date, &timezone);
        days.push(Day {
            date: Some(date),
            show_month: date.day() == 1
                || date.day() == util::month_days(date.year(), date.month()),
            cur_month: date >= month_start && date < month_end,
            occurrences: day_occs,
        });

        date += Duration::days(1);
    }

    let events = Events::new(&state, &locale);
    let tasks = Tasks::new(&state, &locale);

    let html = MonthlyTemplate {
        page,
        locale,
        weekdays,
        month: month_start.format("%B %Y").to_string(),
        prev_month: format!("{}-{}", pyear, pmonth),
        next_month: format!("{}-{}", nyear, nmonth),
        today: Utc::now().with_timezone(&timezone).date_naive(),
        days,
        events,
        tasks,
    }
    .render()
    .context("monthly template")?;

    Ok(Html(html))
}
