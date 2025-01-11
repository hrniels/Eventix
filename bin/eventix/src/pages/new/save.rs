use anyhow::anyhow;
use axum::extract::State;
use axum::response::IntoResponse;
use ical::col::{CalItem, CalStore};
use ical::objects::{
    CalCompType, CalComponent, CalDate, CalEvent, CalTodo, Calendar, UpdatableEventLike,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::comp::CompAction;
use crate::error::HTMLError;
use crate::extract::MultiForm;
use crate::locale::{self, Locale};
use crate::pages::Page;

use super::CompNew;

async fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    store: &Arc<Mutex<CalStore>>,
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
    let mut store = store.lock().await;
    let source = store
        .source_mut(&calendar)
        .ok_or_else(|| anyhow!("Unable to find source with id {}", form.calendar))?;

    let uid = Uuid::new_v4();
    let mut path = source.path().clone();
    path.push(format!("{}.ics", uid));
    let mut item = CalItem::new(calendar, path, Calendar::default());

    let mut comp = if form.req.ctype == CalCompType::Event {
        CalComponent::Event(CalEvent::default())
    } else {
        CalComponent::Todo(CalTodo::default())
    };

    comp.set_uid(uid.to_string());
    comp.set_rrule(rrule);
    comp.set_created(CalDate::now());
    form.update(&mut comp, locale);

    item.add_component(comp);
    item.save()?;

    source.add(item);
    Ok(true)
}

pub async fn handler(
    State(state): State<crate::state::State>,
    MultiForm(mut form): MultiForm<CompNew>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let mut page = super::new_page(&state, &form.req).await;

    if action_update(&mut page, &locale, state.store(), &mut form).await? {
        page.add_info(locale.translate("New event was added successfully"));

        // remember the last used calendar
        {
            let mut last_cal = state.last_calendar().lock().await;
            if let Some(e) = last_cal.get_mut(&form.req.ctype) {
                *e = form.calendar.clone();
            } else {
                last_cal.insert(form.req.ctype, form.calendar.clone());
            }
        }
        form = CompNew::new(&form.req, locale.timezone(), Some(form.calendar));
    }

    super::index::content(page, locale, State(state), form).await
}
