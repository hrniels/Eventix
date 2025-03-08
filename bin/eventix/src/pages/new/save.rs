use anyhow::anyhow;
use axum::extract::State;
use axum::response::IntoResponse;
use ical::col::{CalFile, CalStore};
use ical::objects::{CalCompType, CalComponent, CalEvent, CalTodo, Calendar, UpdatableEventLike};
use std::sync::Arc;
use uuid::Uuid;

use crate::comp::CompAction;
use crate::error::HTMLError;
use crate::extract::MultiForm;
use crate::locale::{self, Locale};
use crate::pages::Page;
use crate::state::EventixState;

use super::CompNew;

async fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    store: &mut CalStore,
    form: &mut CompNew,
) -> anyhow::Result<bool> {
    if !form.check(page, locale, form.req.ctype) {
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
    let dir = store
        .directory_mut(&calendar)
        .ok_or_else(|| anyhow!("Unable to find directory with id {}", form.calendar))?;

    let uid = Uuid::new_v4();
    let mut path = dir.path().clone();
    path.push(format!("{}.ics", uid));
    let mut file = CalFile::new(calendar, path, Calendar::default());

    let mut comp = if form.req.ctype == CalCompType::Event {
        CalComponent::Event(CalEvent::new(uid))
    } else {
        CalComponent::Todo(CalTodo::new(uid))
    };

    comp.set_rrule(rrule);
    form.update(&mut comp, locale);

    file.add_component(comp);
    file.save()?;

    dir.add_file(file);
    Ok(true)
}

pub async fn handler(
    State(state): State<EventixState>,
    MultiForm(mut form): MultiForm<CompNew>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let mut page = super::new_page(&state, &form.req).await;

    {
        let mut state = state.lock().await;
        if action_update(&mut page, &locale, state.store_mut(), &mut form).await? {
            page.add_info(locale.translate("New event was added successfully"));

            // remember the last used calendar
            state.set_last_calendar(form.req.ctype, form.calendar.clone());

            form = CompNew::new(&form.req, locale.timezone(), Some(form.calendar));
        }
    }

    super::index::content(page, locale, State(state), form).await
}
