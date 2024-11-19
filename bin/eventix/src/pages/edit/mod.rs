mod delete;
mod index;
mod update;

use axum::{
    routing::{get, post},
    Router,
};
use chrono_tz::Tz;
use ical::col::Occurrence;
use ical::objects::EventLike;
use serde::{Deserialize, Serialize};

use crate::comps::{datetimerange::DateTimeRange, recur::RecurRequest};

use super::{Breadcrumb, Page};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Update {
    #[serde(flatten)]
    req: Request,
    summary: String,
    location: String,
    description: String,
    rrule: RecurRequest,
    start_end: DateTimeRange,
}

impl Update {
    pub fn new_from_occurrence(req: Request, occ: &Occurrence, tz: &Tz) -> Self {
        Self {
            req,
            summary: occ.summary().cloned().unwrap_or(String::from("")),
            location: occ.location().cloned().unwrap_or(String::from("")),
            description: occ.description().cloned().unwrap_or(String::from("")),
            rrule: RecurRequest::from_rrule(occ.rrule()),
            start_end: DateTimeRange::new_from_caldate(
                Some(occ.occurrence_startdate()),
                occ.occurrence_enddate(),
                tz,
            ),
        }
    }
}

pub fn new_page(req: &Request) -> Page {
    let mut page = Page::new(path().to_string());
    let name = if req.rid.is_some() {
        "Edit occurrence"
    } else {
        "Edit series"
    };
    page.add_breadcrumb(Breadcrumb::new(
        format!("{}?{}", path(), serde_qs::to_string(req).unwrap()),
        name,
    ));
    page
}

pub fn path() -> &'static str {
    "/edit"
}

pub fn router(state: crate::state::State) -> Router {
    Router::new()
        .route("/", get(self::index::handler))
        .route("/update", post(self::update::handler))
        .route("/delete", get(self::delete::handler))
        .with_state(state)
}
