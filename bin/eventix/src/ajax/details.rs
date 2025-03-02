use anyhow::{Context, Result};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::{routing::get, Router};
use ical::col::CalDir;
use ical::objects::EventLike;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use ical::objects::{CalCompType, CalDate};

use crate::comps::organizer::OrganizerTemplate;
use crate::error::HTMLError;
use crate::html::{self, filters};
use crate::locale::{self, DateFlags, Locale};

use crate::objects::DayOccurrence;

pub fn router(state: crate::state::State) -> Router {
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
}

async fn handler(
    State(state): State<crate::state::State>,
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

    let store = state.store().lock().await;

    let occ = store
        .occurrence_by_id(&req.uid, rid.as_ref(), locale.timezone())
        .context(format!(
            "Unable to find occurrence with uid '{}' and rid '{:?}'",
            &req.uid, rid
        ))?;
    let day_occ = DayOccurrence::new(&occ);
    let dir = store.directory(occ.directory()).unwrap();

    let html = DetailsTemplate {
        org: occ
            .organizer()
            .map(|org| OrganizerTemplate::new(locale.clone(), org)),
        occ: day_occ,
        locale,
        dir,
    }
    .render()
    .context("details template")?;

    Ok(Json(Response { html }))
}
