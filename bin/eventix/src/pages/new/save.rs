use anyhow::anyhow;
use axum::extract::State;
use axum::response::IntoResponse;
use ical::col::{CalItem, CalStore};
use ical::objects::{CalComponent, CalDate, CalEvent, Calendar, UpdatableEventLike};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::error::HTMLError;
use crate::extract::MultiForm;
use crate::locale::{self, Locale};
use crate::pages::Page;

use super::Save;

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
    store: &Arc<Mutex<CalStore>>,
    form: &mut Save,
) -> anyhow::Result<bool> {
    if form.summary.is_empty() {
        page.add_error(locale.translate("Summary cannot be empty."));
        return Ok(false);
    }

    let Some(start) = form.start_end.from(locale) else {
        page.add_error(locale.translate("Please specify the start date/time."));
        return Ok(false);
    };
    let Some(end) = form.start_end.to(locale) else {
        page.add_error(locale.translate("Please specify the end date/time."));
        return Ok(false);
    };

    let rrule = match form.rrule.to_rrule() {
        Ok(rrule) => rrule,
        Err(e) => {
            page.add_error(e);
            return Ok(false);
        }
    };

    let mut store = store.lock().unwrap();
    let source = store
        .source_mut(form.calendar)
        .ok_or_else(|| anyhow!("Unable to find source with id {}", form.calendar))?;

    let uid = Uuid::new_v4();
    let mut path = source.path().clone();
    path.push(format!("{}.ics", uid));
    let mut item = CalItem::new(form.calendar, path, Calendar::default());

    let mut comp = CalComponent::Event(CalEvent::default());
    comp.set_uid(uid.to_string());
    comp.set_summary(nonempty_or_none(form.summary.clone()));
    comp.set_location(nonempty_or_none(form.location.clone()));
    comp.set_description(nonempty_or_none(form.description.clone()));
    comp.set_start(Some(start));
    if let Some(ev) = comp.as_event_mut() {
        ev.set_end(Some(end));
    } else {
        comp.as_todo_mut().unwrap().set_due(Some(end));
    }
    comp.set_rrule(rrule);

    comp.set_created(CalDate::now());
    comp.set_last_modified(CalDate::now());
    comp.set_stamp(CalDate::now());

    item.add_component(comp);
    item.save()?;

    source.add(item);
    Ok(true)
}

pub async fn handler(
    State(state): State<crate::state::State>,
    MultiForm(mut form): MultiForm<Save>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let mut page = super::new_page();

    if action_update(&mut page, &locale, state.store(), &mut form)? {
        page.add_info(locale.translate("New event was added successfully"));
        form = Save::new(locale.timezone());
    }

    super::index::content(page, locale, State(state), form).await
}
