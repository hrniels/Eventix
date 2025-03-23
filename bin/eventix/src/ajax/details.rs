use anyhow::{Context, Result};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::{routing::get, Router};
use ical::col::CalDir;
use ical::objects::{CalAlarm, EventLike};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use ical::objects::{CalCompType, CalDate};

use crate::comps::organizer::OrganizerTemplate;
use crate::error::HTMLError;
use crate::html::{self, filters};
use crate::locale::{self, DateFlags, Locale};

use crate::objects::DayOccurrence;
use crate::state::EventixState;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/details", get(handler))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
}

#[derive(Debug, Serialize)]
struct Response {
    html: String,
}

#[derive(Template)]
#[template(path = "ajax/details.htm")]
struct DetailsTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    dir: &'a CalDir,
    occ: DayOccurrence<'a>,
    org: Option<OrganizerTemplate<'a>>,
    effective_alarms: Option<Vec<CalAlarm>>,
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();

    let rid = if let Some(rid) = req.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {}", rid))?,
        )
    } else {
        None
    };

    let state = state.lock().await;

    let occ = state
        .store()
        .occurrence_by_id(&req.uid, rid.as_ref(), locale.timezone())
        .context(format!(
            "Unable to find occurrence with uid '{}' and rid '{:?}'",
            &req.uid, rid
        ))?;

    let effective_alarms = state.personal_alarms().effective_alarms(&occ);
    let day_occ = DayOccurrence::new(&occ, effective_alarms.is_some());
    let dir = state.store().directory(occ.directory()).unwrap();

    let html = DetailsTemplate {
        org: occ
            .organizer()
            .map(|org| OrganizerTemplate::new(locale.clone(), org)),
        occ: day_occ,
        locale,
        dir,
        effective_alarms,
    }
    .render()
    .context("details template")?;

    Ok(Json(Response { html }))
}
