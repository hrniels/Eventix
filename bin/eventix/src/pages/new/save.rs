use anyhow::anyhow;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use eventix_ical::col::CalFile;
use eventix_ical::objects::{
    CalCompType, CalComponent, CalEvent, CalTimeZone, CalTodo, Calendar, UpdatableEventLike,
};
use eventix_locale::Locale;
use eventix_state::EventixState;
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

use crate::extract::MultiForm;
use crate::objects::CompAction;
use crate::pages::{Page, error::HTMLError};

use super::{CompNew, Request};

async fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    state: &mut eventix_state::State,
    form: &mut CompNew,
    req: &Request,
) -> anyhow::Result<bool> {
    if !form.check(page, locale, req.ctype) {
        return Ok(false);
    }

    let rrule = match form.rrule.to_rrule() {
        Ok(rrule) => rrule,
        Err(e) => {
            page.add_error(e);
            return Ok(false);
        }
    };

    let calendar = Arc::from(form.calendar.clone());
    let cal_settings = state.settings().calendar(&calendar).unwrap();
    let organizer = cal_settings.build_organizer();
    let alarm_type = cal_settings.alarms().clone();

    let uid = Uuid::new_v4();
    let mut comp = if req.ctype == CalCompType::Event {
        CalComponent::Event(CalEvent::new(uid))
    } else {
        CalComponent::Todo(CalTodo::new(uid))
    };

    comp.set_rrule(rrule);
    form.update(
        &calendar,
        &alarm_type,
        &mut comp,
        state.personal_alarms_mut(),
        organizer,
        locale,
    );

    let dir = state
        .store_mut()
        .directory_mut(&calendar)
        .ok_or_else(|| anyhow!("Unable to find directory with id {}", form.calendar))?;

    let mut path = dir.path().clone();
    path.push(format!("{uid}.ics"));

    let mut cal = Calendar::default();
    // add a VTIMEZONE entry to the calendar with just the name of the timezone to work around a
    // problem in the interaction of davmail and MS exchange. davmail translates the timezones to
    // in ICS files to different names for compatibility reasons (I guess) by taking the timezone
    // from the VTIMEZONE entries and setting the same timezone in all DTSTART, DTEND, etc.
    // properties. If there is no VTIMEZONE entry, this translation isn't done, but davmail inserts
    // a new VTIMEZONE entry, which then has a different timezone name than in DTSTART etc.. This
    // apparently is only a problem when updating ICS files, not when creating them. Therefore, it
    // worked often fine so far, because MS exchange added the VTIMEZONE entry for us. This however
    // does not always happen, so that we run into this issue.
    //
    // A working fix is to add a VTIMEZONE entry with just the timezone name and let davmail/MS
    // exchange add the daylight/standard information for us.
    cal.add_timezone(CalTimeZone::new(locale.timezone().name().to_string()));

    let mut file = CalFile::new(calendar, path, cal);

    file.add_component(comp);
    file.save()?;

    dir.add_file(file);
    Ok(true)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
    MultiForm(mut form): MultiForm<CompNew>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = state.lock().await.settings().locale();
    let mut page = super::new_page(&state, &req).await;

    {
        let mut state = state.lock().await;
        if action_update(&mut page, &locale, &mut state, &mut form, &req).await? {
            page.add_info(locale.translate("New event was added successfully"));

            // remember the last used calendar
            let misc = state.misc_mut();
            misc.set_last_calendar(req.ctype, form.calendar.clone());
            if let Err(e) = misc.write_to_file() {
                warn!("Unable to misc state: {}", e);
            }

            form = CompNew::new(&req, locale.timezone(), Some(form.calendar));
        }
    }

    super::index::content(page, locale, State(state), form, req).await
}
