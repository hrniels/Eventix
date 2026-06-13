// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, anyhow};
use axum::{
    Json,
    extract::{Query, State},
};
use chrono::Duration;
use chrono_tz::Tz;
use eventix_ical::col::CalFile;
use eventix_ical::objects::{
    CalCompType, CalComponent, CalDate, CalDateTime, CalEvent, CalTodo, Calendar, EventLike,
    UpdatableEventLike,
};
use eventix_locale::Locale;
use eventix_state::EventixState;
use std::sync::Arc;
use uuid::Uuid;

use crate::api::items::edit::EditMode;
use crate::extract::MultiForm;
use crate::util;
use crate::{
    api::{HTMLResponse, JsonError},
    pages::Page,
};

use super::{CompAction, CompEdit, Request};

fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    state: &mut eventix_state::State,
    form: &mut CompEdit,
    req: &mut Request,
) -> anyhow::Result<(bool, Option<String>)> {
    let (calendar, alarm_type, organizer) = {
        let file = state
            .store()
            .file_by_id(&req.uid)
            .context(format!("Unable to find component with uid '{}'", req.uid))?;
        let calendar = form.calendar.as_ref().unwrap_or(file.directory());
        let (col_settings, cal_settings) = state.settings().calendar(calendar).unwrap();
        let organizer = col_settings.build_organizer();
        (calendar.clone(), cal_settings.alarms().clone(), organizer)
    };

    let (store, personal_alarms) = state.store_and_alarms_mut();

    let file = store
        .file_by_id_mut(&req.uid)
        .context(format!("Unable to find component with uid '{}'", req.uid))?;
    let ctx = file.calendar().date_context();

    let last_modified = util::system_time_stamp(file.last_modified()?);
    if last_modified > form.edit_start {
        page.add_error(format!(
            "This component has been modified. Please restart the editing."
        ));
        return Ok((false, None));
    }

    let rid = if let Some(ref rid) = req.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: '{rid}'"))?,
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
        return Ok((false, None));
    }

    let event_tz = form.start_end.effective_timezone(locale);

    let rrule = if req.mode == EditMode::Occurrence {
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
        let res = form.rrule.as_ref().map(|rr| {
            let start = form.start_end().as_caldates(locale, ctype.into()).0;
            rr.to_rrule(start.as_ref())
        });
        match res {
            None => None,
            Some(Ok(rrule)) => rrule,
            Some(Err(e)) => {
                page.add_error(e);
                return Ok((false, None));
            }
        }
    };

    let new_cal = if req.mode != EditMode::Occurrence {
        form.calendar
            .clone()
            .ok_or_else(|| anyhow!("Calendar not specified"))?
    } else {
        calendar
    };

    let new_uid = if req.mode == EditMode::Following {
        let rid = rid.unwrap();

        // end the series before this occurrence
        let old_start = {
            let base = file
                .component_with_mut(|c| c.uid() == &req.uid && c.rid().is_none())
                .context("Unable to find base component")?;
            let mut old_rrule = base.rrule().unwrap().clone();
            let old_start = base.start().unwrap().clone();
            let until = match &old_start {
                CalDate::Date(_, _) => {
                    CalDate::Date(rid.as_naive_date().pred_opt().unwrap(), ctype.into())
                }
                CalDate::DateTime(_) => {
                    let utc = rid.to_utc();
                    let dt = match utc {
                        CalDate::DateTime(CalDateTime::Utc(dt)) => dt,
                        _ => unreachable!(),
                    };
                    CalDate::DateTime(CalDateTime::Utc(dt - Duration::seconds(1)))
                }
            };
            old_rrule.set_until(until);
            base.set_rrule(Some(old_rrule));
            base.touch();
            old_start
        };

        // delete all future overwrites
        file.calendar_mut().delete_components(|c| {
            if c.uid() != &req.uid {
                return false;
            }
            if let Some(crid) = c.rid() {
                crid >= &rid
            } else {
                false
            }
        });

        // build new event/TODO
        let calendar = Arc::new(new_cal);
        let uid = Uuid::new_v4();
        let mut comp = if ctype == CalCompType::Event {
            CalComponent::Event(CalEvent::new(uid))
        } else {
            CalComponent::Todo(CalTodo::new(uid))
        };

        // set properties from forms
        comp.set_rrule(rrule);
        form.update(
            &calendar,
            &alarm_type,
            &mut comp,
            personal_alarms,
            organizer,
            locale,
        )?;

        // update old event/TODO; check if there are no occurrences left
        let start = ctx.date(&old_start).start_in(locale.timezone());
        let end = ctx.date(&rid).end_in(locale.timezone());
        if file
            .occurrences_between(start, end, |_| true)
            .next()
            .is_none()
        {
            // no occurrences left -> remove UID
            let old_dir = file.directory().clone();
            let dir = state
                .store_mut()
                .directory_mut(&old_dir)
                .map_err(anyhow::Error::from)?;
            dir.delete_by_uid(&req.uid)?;
        } else {
            // just update the file
            file.save()?;
        }

        // save to file
        let dir = state
            .store_mut()
            .directory_mut(&calendar)
            .map_err(anyhow::Error::from)?;

        let mut path = dir.path().clone();
        path.push(format!("{uid}.ics"));

        let mut cal = Calendar::default();
        cal.add_component(comp);
        cal.populate_timezones();

        let new_file = CalFile::new(calendar, path, cal);
        new_file.save()?;

        dir.add_file(new_file).map_err(anyhow::Error::from)?;

        Some(uid.to_string())
    } else {
        if let Some(comp) =
            file.component_with_mut(|c| c.uid() == &req.uid && c.rid() == rid.as_ref())
        {
            // For EditMode::Series, shift all overwrite RECURRENCE-IDs before applying the rest
            // of the form data. This ensures overwrites remain findable after a time change.
            if req.mode == EditMode::Series && comp.is_recurrent() {
                let dtype = comp.ctype().into();
                let (new_start, new_end) = form.start_end.as_caldates(locale, dtype);
                let should_shift_series =
                    new_start.as_ref() != comp.start() || new_end.as_ref() != comp.end_or_due();
                if let Some(new_start) = new_start
                    && should_shift_series
                {
                    file.change_series_range(&req.uid, new_start, new_end)
                        .context("Shifting overwrite RIDs failed")?;
                }
            }

            let comp = file
                .component_with_mut(|c| c.uid() == &req.uid && c.rid() == rid.as_ref())
                .unwrap();
            form.update(
                &new_cal,
                &alarm_type,
                comp,
                personal_alarms,
                organizer,
                locale,
            )?;
            if rid.is_none() {
                comp.set_rrule(rrule);
            }
        } else {
            let comp = file.component_with(|c| c.uid() == &req.uid).unwrap();
            if !comp.is_recurrent() {
                return Err(anyhow!("Component '{}' is not recurrent", req.uid));
            }

            let tz: Tz = event_tz
                .parse()
                .map_err(|_| anyhow!("Invalid timezone: {}", event_tz))?;
            let rid = rid.unwrap();

            let normalized_rid = file
                .create_overwrite(&req.uid, rid, &tz, |_, c| {
                    form.update(&new_cal, &alarm_type, c, personal_alarms, organizer, locale)
                })
                .context("Creating overwrite failed")?;

            if req.mode == EditMode::Occurrence {
                req.rid = Some(normalized_rid.to_string());
            }
        }

        file.calendar_mut().populate_timezones();

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
                return Ok((true, None));
            }
        }

        file.save()?;
        None
    };

    Ok((true, new_uid))
}

pub async fn content(
    State(state): State<EventixState>,
    Query(mut req): Query<Request>,
    MultiForm(mut form): MultiForm<CompEdit>,
) -> Result<Json<HTMLResponse>, JsonError> {
    let locale = state.lock().await.locale();
    let mut page = Page::new(&state).await;

    let (form, errors) = {
        let mut state = state.lock().await;
        match action_update(&mut page, &locale, &mut state, &mut form, &mut req) {
            Ok((true, Some(uid))) => {
                req.uid = uid;
                req.mode = EditMode::Series;
                req.rid = None;
                (None, Vec::new())
            }
            Ok((true, None)) => {
                return Ok(Json(HTMLResponse::new(String::new())));
            }
            Ok((false, _)) => (Some(form), page.errors().to_vec()),
            Err(e) => return Err(e.into()),
        }
    };

    super::index::content_with(locale, State(state), Query(req), form, errors).await
}
