use anyhow::Context;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use ical::col::CalItem;
use ical::objects::{CalDate, EventLike, UpdatableEventLike};
use serde::Deserialize;
use std::sync::Arc;

use crate::comps::daterange::DateRange;
use crate::error::HTMLError;
use crate::extract::MultiForm;
use crate::locale::{self, Locale};
use crate::pages::Page;

use super::Request;

#[derive(Debug, Deserialize)]
pub struct Update {
    #[serde(flatten)]
    base: Request,
    summary: String,
    location: String,
    description: String,
    start_end: DateRange,
}

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
) -> anyhow::Result<()> {
    let rid = if let Some(ref rid) = form.base.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {}", rid))?,
        )
    } else {
        None
    };

    if form.summary.is_empty() {
        page.add_error(locale.translate("Summary cannot be empty."));
        return Ok(());
    }

    let Some(start) = form.start_end.from(locale) else {
        page.add_error(locale.translate("Please specify the start date/time."));
        return Ok(());
    };
    let Some(end) = form.start_end.to(locale) else {
        page.add_error(locale.translate("Please specify the end date/time."));
        return Ok(());
    };

    if form.base.rid.is_some() {
        let base = item
            .component_with(|c| c.uid() == &form.base.uid && c.rid().is_none())
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
    }

    let comp = item
        .component_with_mut(|c| c.uid() == &form.base.uid && c.rid() == rid.as_ref())
        .context("Unable to find component")?;

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

    item.save()
}

pub async fn handler(
    State(state): State<crate::state::State>,
    MultiForm(mut form): MultiForm<Update>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let mut page = super::new_page(&form.base);

    {
        let mut store = state.store().lock().unwrap();

        let item = store.item_by_id_mut(&form.base.uid).context(format!(
            "Unable to find component with uid '{}'",
            form.base.uid
        ))?;

        action_update(&mut page, &locale, item, &mut form)?;
    }

    super::index::content(page, locale, State(state), Query(form.base)).await
}
