use anyhow::Context;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use ical::col::CalItem;
use ical::objects::{CalComponent, CalDate, CalDateTime, CalEvent, EventLike, UpdatableEventLike};
use std::sync::Arc;

use crate::error::HTMLError;
use crate::extract::MultiForm;
use crate::locale::{self, Locale};
use crate::pages::Page;

use super::Update;

fn nonempty_or_none(val: String) -> Option<String> {
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    item: &mut CalItem,
    form: &mut Update,
) -> anyhow::Result<bool> {
    let rid = if let Some(ref rid) = form.req.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {}", rid))?,
        )
    } else {
        None
    };

    if form.summary.is_empty() {
        page.add_error(locale.translate("Summary cannot be empty."));
        return Ok(false);
    }

    let Some(start) = form.start_end.from_as_caldate(locale) else {
        page.add_error(locale.translate("Please specify the start date/time."));
        return Ok(false);
    };
    let Some(end) = form.start_end.to_as_caldate(locale) else {
        page.add_error(locale.translate("Please specify the end date/time."));
        return Ok(false);
    };

    let rrule = if form.req.rid.is_some() {
        let base = item
            .component_with(|c| c.uid() == &form.req.uid && c.rid().is_none())
            .context("Unable to find base component")?;

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
        match form.rrule.to_rrule() {
            Ok(rrule) => rrule,
            Err(e) => {
                page.add_error(e);
                return Ok(false);
            }
        }
    };

    if let Some(comp) =
        item.component_with_mut(|c| c.uid() == &form.req.uid && c.rid() == rid.as_ref())
    {
        update_component(comp, form, start, end);
        if rid.is_none() {
            comp.set_rrule(rrule);
        }
    } else {
        let mut comp = CalComponent::Event(CalEvent::default());
        update_component(&mut comp, form, start, end);
        comp.set_uid(form.req.uid.clone());
        let start = CalDate::DateTime(CalDateTime::Timezone(
            rid.as_ref()
                .unwrap()
                .as_start_with_tz(locale.timezone())
                .naive_local(),
            locale.timezone().name().to_string(),
        ));
        comp.set_start(Some(start));
        comp.set_rid(rid);
        item.add_component(comp);
    }

    item.save()?;
    Ok(true)
}

fn update_component(comp: &mut CalComponent, form: &Update, start: CalDate, end: CalDate) {
    comp.set_summary(nonempty_or_none(form.summary.clone()));
    comp.set_location(nonempty_or_none(form.location.clone()));
    comp.set_description(nonempty_or_none(form.description.clone()));
    comp.set_start(Some(start));
    if let Some(ev) = comp.as_event_mut() {
        ev.set_end(Some(end));
    } else {
        comp.as_todo_mut().unwrap().set_due(Some(end));
    }

    comp.set_last_modified(CalDate::now());
    comp.set_stamp(CalDate::now());
}

pub async fn handler(
    State(state): State<crate::state::State>,
    MultiForm(mut form): MultiForm<Update>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let mut page = super::new_page(&form.req);

    let req = form.req.clone();
    let form = {
        let mut store = state.store().lock().unwrap();

        let item = store.item_by_id_mut(&form.req.uid).context(format!(
            "Unable to find component with uid '{}'",
            form.req.uid
        ))?;

        if action_update(&mut page, &locale, item, &mut form)? {
            None
        } else {
            Some(form)
        }
    };

    super::index::content(page, locale, State(state), Query(req), form).await
}
