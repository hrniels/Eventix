use anyhow::{Context, Result};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json};
use axum::{Router, routing::get};
use eventix_ical::col::CalDir;
use eventix_ical::objects::{CalCompType, CalDate, CalPartStat, EventLike};
use eventix_locale::Locale;
use eventix_state::{CalendarAlarmType, EventixState};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::JsonError;
use crate::comps::editmodes::EditModesTemplate;
use crate::comps::{
    editalarm::EditAlarmTemplate, organizer::OrganizerTemplate, partstat::PartStatTemplate,
};
use crate::html::{self, filters};
use crate::objects::DayOccurrence;

pub fn router(state: EventixState) -> Router {
    Router::new()
        .route("/details", get(handler))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct Request {
    uid: String,
    rid: Option<CalDate>,
    edit: bool,
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
    personal_alarms: bool,
    alarms: Option<EditAlarmTemplate<'a>>,
    series_partstat: Option<PartStatTemplate>,
    occ_partstat: Option<PartStatTemplate>,
    edit_modes: Option<EditModesTemplate>,
    owner: bool,
}

fn attendee_status<E: EventLike>(
    ev: &E,
    user_mail: Option<&String>,
    owner: bool,
) -> Option<CalPartStat> {
    match (user_mail, owner) {
        (Some(user_mail), false) => ev.attendee_status(user_mail),
        _ => None,
    }
}

async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, JsonError> {
    let state = state.lock().await;
    let locale = state.locale();

    let occ = state
        .store()
        .occurrence_by_id(&req.uid, req.rid.as_ref(), locale.timezone())
        .context(format!(
            "Unable to find occurrence with uid '{}' and rid '{:?}'",
            &req.uid, req.rid
        ))?;

    let (collection, calendar) = state.settings().calendar(occ.directory()).unwrap();
    let alarm_type = calendar.alarms();
    let user_mail = collection.email().map(|e| e.address());
    let has_alarms = state.personal_alarms().has_alarms(&occ, alarm_type);
    let dir = state.store().directory(occ.directory()).unwrap();

    let owner = occ.is_owned_by(user_mail.as_ref());
    let series_stat = attendee_status(occ.base(), user_mail.as_ref(), owner);
    let occ_stat = if occ.is_recurrent() {
        attendee_status(&occ, user_mail.as_ref(), owner)
    } else {
        None
    };

    let day_occ = DayOccurrence::new(&occ, occ_stat, owner, has_alarms);

    let html = DetailsTemplate {
        org: occ
            .organizer()
            .map(|org| OrganizerTemplate::new(locale.clone(), org)),
        dir,
        personal_alarms: matches!(alarm_type, CalendarAlarmType::Personal { .. }),
        alarms: if matches!(alarm_type, CalendarAlarmType::Personal { .. }) || has_alarms {
            Some(EditAlarmTemplate::new(
                locale.clone(),
                &state,
                req.uid.clone(),
                req.rid.clone(),
                req.edit,
            )?)
        } else {
            None
        },
        series_partstat: series_stat.map(|stat| {
            PartStatTemplate::new(
                locale.clone(),
                "series",
                stat,
                req.uid.clone(),
                None,
                occ.is_recurrent(),
            )
        }),
        occ_partstat: occ_stat.map(|stat| {
            PartStatTemplate::new(
                locale.clone(),
                "occurrence",
                stat,
                req.uid.clone(),
                Some(day_occ.rid_str()),
                occ.is_recurrent(),
            )
        }),
        edit_modes: if owner {
            req.rid.map(|rid| {
                EditModesTemplate::new(locale.clone(), "edit", req.uid.clone(), rid.to_string())
            })
        } else {
            None
        },
        occ: day_occ,
        owner,
        locale,
    }
    .render()
    .context("details template")?;

    Ok(Json(Response { html }))
}
