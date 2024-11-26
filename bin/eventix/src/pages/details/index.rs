use anyhow::{Context, Result};
use askama::{Html, MarkupDisplay, Template};
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use ical::col::{CalSource, Occurrence};
use ical::objects::{CalPartStat, CalRole, EventLike};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::Arc;

use ical::objects::{CalAttendee, CalCompType, CalDate};

use crate::error::HTMLError;
use crate::html::filters;
use crate::locale::{self, Locale};

use crate::objects::DayOccurrence;

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
#[template(path = "pages/details.htm")]
struct DetailsTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    source: &'a CalSource,
    occ: DayOccurrence<'a>,
}

impl DetailsTemplate<'_> {
    fn description(occ: &DayOccurrence) -> Option<String> {
        match occ.description().map(|desc| desc.trim()) {
            Some(desc) if !desc.is_empty() => {
                // the problem is that we need to find URLs before translating HTML entities. but
                // if we directly replace URLs with '<a ...>', we will translate the HTML entities
                // in there afterwards. therefore, we use an intermediate step by first marking the
                // URLs by surrounding them with \0 and then we replace this with the actual HTML
                // code later.
                let regex = r"(https?:\/\/)?(www\.)?[-a-zA-Z0-9@:%._\+~#=]{1,256}\.[a-zA-Z0-9()]{2,6}\b([-a-zA-Z0-9()@:;%_\+.~#?&/=]*)";
                let re = Regex::new(regex).unwrap();
                let desc = re.replace_all(desc, "\0$0\0");

                // now replace HTML entities etc.
                let desc = MarkupDisplay::new_unsafe(desc, Html);
                let desc = format!("{}", desc);
                let desc = desc.replace('\n', "<br>");

                // finally replace URLs with proper links
                let re = Regex::new("\0(.*?)\0").unwrap();
                Some(
                    re.replace_all(&desc, |caps: &Captures| {
                        // a few heuristics here to prefix URLs with the right protocol
                        if caps[1].starts_with("http:")
                            || caps[1].starts_with("https:")
                            || caps[1].starts_with("mailto:")
                        {
                            format!("<a href=\"{0}\">{0}</a>", &caps[1])
                        } else if caps[1].contains('@') {
                            format!("<a href=\"mailto:{0}\">{0}</a>", &caps[1])
                        } else {
                            format!("<a href=\"https://{0}\">{0}</a>", &caps[1])
                        }
                    })
                    .to_string(),
                )
            }
            _ => None,
        }
    }
}

fn attendee_icon(att: &CalAttendee) -> String {
    let role = match att.role() {
        Some(CalRole::Required) => "-fill",
        Some(CalRole::Optional) => "",
        _ => "",
    };

    let status = match att.part_stat() {
        Some(CalPartStat::Accepted) => "-check",
        Some(CalPartStat::Declined) => "-slash",
        Some(CalPartStat::Tentative) => "-exclamation",
        _ => "",
    };

    format!("bi bi-person{}{}", role, status)
}

fn attendees_sorted(occ: &Occurrence<'_>) -> Vec<CalAttendee> {
    if let Some(atts) = occ.attendees() {
        let mut att = atts.to_vec();
        att.sort_by(|a, b| match (a.common_name(), b.common_name()) {
            (Some(cn1), Some(cn2)) => cn1.cmp(cn2),
            _ => Ordering::Equal,
        });
        att
    } else {
        vec![]
    }
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

    let store = state.store().lock().await;

    let occ = store
        .occurrence_by_id(&req.uid, Some(&rid), locale.timezone())
        .context(format!(
            "Unable to find occurrence with uid '{}' and rid '{:?}'",
            &req.uid, req.rid
        ))?;
    let occ = DayOccurrence::new(&occ);
    let source = store.source(occ.source()).unwrap();

    let html = DetailsTemplate {
        locale,
        source,
        occ,
    }
    .render()
    .context("event template")?;

    Ok(Json(Response { html }))
}
