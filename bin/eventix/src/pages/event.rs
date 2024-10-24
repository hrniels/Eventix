use anyhow::{Context, Result};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use ical::col::{CalSource, Occurrence};
use ical::objects::{CalPartStat, CalRole, EventLike};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::Arc;

use ical::objects::{CalAttendee, CalDate};

use crate::error::HTMLError;
use crate::html::filters;
use crate::locale::{self, Locale};

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: String,
}

#[derive(Debug, Serialize)]
struct Response {
    html: String,
}

#[derive(Template)]
#[template(path = "pages/event.htm")]
struct EventTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    source: &'a CalSource,
    occ: Occurrence<'a>,
}

fn attendee_icon(att: &CalAttendee) -> String {
    let role = match att.role() {
        Some(CalRole::Required) => "-fill",
        Some(CalRole::Optional) | _ => "",
    };

    let status = match att.part_stat() {
        Some(CalPartStat::Accepted) => "-check",
        Some(CalPartStat::Declined) => "-slash",
        Some(CalPartStat::Tentative) => "-exclamation",
        _ => "",
    };

    format!("bi bi-person{}{}", role, status)
}

fn attendees_sorted<'a>(occ: &Occurrence<'a>) -> Vec<CalAttendee> {
    let mut att = occ.attendees().to_vec();
    att.sort_by(|a, b| match (a.common_name(), b.common_name()) {
        (Some(cn1), Some(cn2)) => cn1.cmp(&cn2),
        _ => Ordering::Equal,
    });
    att
}

fn attendee_title(att: &CalAttendee) -> String {
    let mut res = String::new();
    if let Some(role) = att.role() {
        res.push_str(&format!("{:?}", role));
    }
    if let Some(status) = att.part_stat() {
        if att.role().is_some() {
            res.push_str(", ");
        }
        res.push_str(&format!("{:?}", status));
    }
    res
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();

    let rid = req
        .rid
        .parse::<CalDate>()
        .context(format!("Invalid rid date: {}", req.rid))?;

    let occ = state
        .store()
        .occurrence_by_id(&req.uid, &rid, locale.timezone())
        .context(format!(
            "Unable to find occurrence with uid '{}' and rid '{:?}'",
            &req.uid, req.rid
        ))?;
    let source = state.store().source(occ.source()).unwrap();

    let html = EventTemplate {
        locale,
        source,
        occ,
    }
    .render()
    .context("event template")?;

    Ok(Json(Response { html }))
}

pub fn path() -> &'static str {
    "/event"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new().route("/", get(handler)).with_state(state)
}
