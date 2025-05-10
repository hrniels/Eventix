use anyhow::{Context, anyhow};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use ical::objects::{CalDate, CalTimeZone, EventLike, UpdatableEventLike};
use std::sync::Arc;

use crate::error::HTMLError;
use crate::extract::MultiForm;
use crate::locale::{self, Locale};
use crate::pages::Page;
use crate::state::EventixState;
use crate::util;

use super::{CompAction, CompEdit, Request};

fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    state: &mut crate::state::State,
    form: &mut CompEdit,
    req: &Request,
) -> anyhow::Result<bool> {
    let (calendar, alarm_type, organizer) = {
        let file = state
            .store()
            .file_by_id(&req.uid)
            .context(format!("Unable to find component with uid '{}'", req.uid))?;
        let calendar = form.calendar.as_ref().unwrap_or(file.directory());
        let cal_settings = state.settings().calendar(calendar).unwrap();
        let organizer = cal_settings.build_organizer();
        (calendar.clone(), cal_settings.alarms().clone(), organizer)
    };

    let (store, personal_alarms) = state.store_and_alarms_mut();

    let file = store
        .files_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;

    let last_modified = util::system_time_stamp(file.last_modified()?);
    if last_modified > form.edit_start {
        page.add_error(format!(
            "This component has been modified. Please <a href=\"/edit?{}\">restart</a> the editing.",
            serde_qs::to_string(&req).unwrap()
        ));
        return Ok(false);
    }

    let rid = if let Some(ref rid) = req.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {}", rid))?,
        )
    } else {
        None
    };

    let base = file
        .component_with(|c| c.uid() == &req.uid && c.rid().is_none())
        .context("Unable to find base component")?;
    let ctype = base.ctype();

    if !base.is_owned_by(organizer.as_ref().map(|o| o.address())) {
        return Err(anyhow!("No edit permission"));
    }

    if !form.check(page, locale, ctype) {
        return Ok(false);
    }

    let rrule = if req.rid.is_some() {
        // inherit from base if we can
        if Some(&form.summary) == base.summary() {
            form.summary.clear();
        }
        if Some(&form.location) == base.location() {
            form.location.clear();
        }
        if Some(&form.description) == base.description() {
            form.description.clear();
        }
        None
    } else {
        match form.rrule.as_ref().map(|rr| rr.to_rrule()) {
            None => None,
            Some(Ok(rrule)) => rrule,
            Some(Err(e)) => {
                page.add_error(e);
                return Ok(false);
            }
        }
    };

    let new_cal = if req.rid.is_none() {
        form.calendar
            .clone()
            .ok_or_else(|| anyhow!("Calendar not specified"))?
    } else {
        calendar
    };

    if let Some(comp) = file.component_with_mut(|c| c.uid() == &req.uid && c.rid() == rid.as_ref())
    {
        form.update(
            &new_cal,
            &alarm_type,
            comp,
            personal_alarms,
            organizer,
            locale,
        );
        if rid.is_none() {
            comp.set_rrule(rrule);
        }
    } else {
        let comp = file.component_with(|c| c.uid() == &req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", req.uid));
        }

        file.create_overwrite(&req.uid, rid.unwrap(), locale.timezone(), |_, c| {
            form.update(&new_cal, &alarm_type, c, personal_alarms, organizer, locale);
        })
        .context("Creating overwrite failed")?;
    }

    // add "empty" timezone information as a workaround for davmail/exchange
    // see comment in new/save.rs for details.
    if file.calendar().timezones().is_empty() {
        file.calendar_mut()
            .add_timezone(CalTimeZone::new(locale.timezone().name().to_string()));
    }

    // should we move the file to a different directory?
    if req.rid.is_none() {
        let cal = form
            .calendar
            .as_ref()
            .ok_or_else(|| anyhow!("Calendar not specified"))?;
        if *cal != **file.directory() {
            let path = file.path().clone();
            let src = file.directory().clone();
            state
                .store_mut()
                .switch_directory(path, &src, &Arc::new(cal.to_string()))?;
            return Ok(true);
        }
    }

    file.save()?;
    Ok(true)
}

pub async fn handler(
    State(state): State<EventixState>,
    Query(req): Query<Request>,
    MultiForm(mut form): MultiForm<CompEdit>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let mut page = super::new_page(&state).await;

    let form = {
        let mut state = state.lock().await;
        if action_update(&mut page, &locale, &mut state, &mut form, &req)? {
            None
        } else {
            Some(form)
        }
    };

    super::index::content(page, locale, State(state), Query(req), form).await
}
