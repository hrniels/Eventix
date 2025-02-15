use anyhow::{Context, Result};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use chrono::offset::LocalResult;
use chrono::{DateTime, Duration, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use ical::objects::{CalCompType, CalDate, CalTodoStatus, EventLike};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::HTMLError;
use crate::html::{self, filters};
use crate::locale::{self, Locale};

use crate::objects::DayOccurrence;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
enum Direction {
    Backwards,
    Forward,
}

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    date: String,
    dir: Direction,
}

#[derive(Debug, Serialize)]
struct Response {
    html: String,
    date: Option<String>,
}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/occlist", get(handler))
        .with_state(state)
}

#[derive(Template)]
#[template(path = "pages/occlist.htm")]
struct OccListTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    occs: Vec<DayOccurrence<'a>>,
}

fn min_datetime(timezone: Tz) -> DateTime<Tz> {
    let mut naive = NaiveDateTime::MIN;
    loop {
        match timezone.from_local_datetime(&naive) {
            LocalResult::Single(date) => break date,
            _ => naive = naive + Duration::days(1),
        }
    }
}

fn max_datetime(timezone: Tz) -> DateTime<Tz> {
    let mut naive = NaiveDateTime::MAX;
    loop {
        match timezone.from_local_datetime(&naive) {
            LocalResult::Single(date) => break date,
            _ => naive = naive - Duration::days(1),
        }
    }
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    const COUNT: usize = 5;

    let locale = locale::default();

    let date = req
        .date
        .parse::<CalDate>()
        .context(format!("Invalid date: {}", req.date))?
        .as_start_with_tz(locale.timezone());

    let store = state.store().lock().await;
    let item = store
        .item_by_id(&req.uid)
        .context(format!("Unable to find item with uid {}", req.uid))?;

    let occs: Vec<_> = match req.dir {
        Direction::Forward => {
            let start = date + Duration::seconds(1);
            let end = max_datetime(*locale.timezone());
            item.occurrences_within(start, end)
                .take(COUNT + 1)
                .collect()
        }
        Direction::Backwards => {
            let start = min_datetime(*locale.timezone());
            let end = date;
            let occs = item.occurrences_within(start, end).collect::<Vec<_>>();
            occs[occs.len().saturating_sub(COUNT + 1)..].to_vec()
        }
    };

    let more = occs.len() > COUNT;
    let occs: Vec<_> = match req.dir {
        Direction::Forward => occs
            .iter()
            .take(COUNT)
            .map(|o| DayOccurrence::new(o))
            .collect(),
        Direction::Backwards => occs
            .iter()
            .skip(if more { 1 } else { 0 })
            .map(|o| DayOccurrence::new(o))
            .collect(),
    };

    let date = if more {
        match req.dir {
            Direction::Forward => occs.iter().last().and_then(|l| l.occurrence_end()),
            Direction::Backwards => occs.iter().next().and_then(|l| l.occurrence_start()),
        }
        .map(|d| d.to_utc().format("%Y%m%dT%H%M%SZ").to_string())
    } else {
        None
    };

    let html = OccListTemplate { occs, locale }
        .render()
        .context("details template")?;

    Ok(Json(Response { html, date }))
}
