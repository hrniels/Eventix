use anyhow::{anyhow, Context};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use ical::col::CalItem;
use ical::objects::{CalDate, EventLike, UpdatableEventLike};
use std::sync::Arc;

use crate::error::HTMLError;
use crate::extract::MultiForm;
use crate::locale::{self, Locale};
use crate::pages::Page;

use super::{CompAction, CompEdit};

fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    item: &mut CalItem,
    form: &mut CompEdit,
) -> anyhow::Result<bool> {
    let rid = if let Some(ref rid) = form.req.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {}", rid))?,
        )
    } else {
        None
    };

    let base = item
        .component_with(|c| c.uid() == &form.req.uid && c.rid().is_none())
        .context("Unable to find base component")?;
    let ctype = base.ctype();

    if !form.check(page, locale, ctype) {
        return Ok(false);
    }

    let rrule = if form.req.rid.is_some() {
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

    if let Some(comp) =
        item.component_with_mut(|c| c.uid() == &form.req.uid && c.rid() == rid.as_ref())
    {
        form.update(comp, locale);
        if rid.is_none() {
            comp.set_rrule(rrule);
        }
    } else {
        let comp = item.component_with(|c| c.uid() == &form.req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", form.req.uid).into());
        }

        item.overwrite_component(rid.unwrap(), locale.timezone(), |c| {
            form.update(c, locale);
        });
    }

    item.save()?;
    Ok(true)
}

pub async fn handler(
    State(state): State<crate::state::State>,
    MultiForm(mut form): MultiForm<CompEdit>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let mut page = super::new_page(&state).await;

    let req = form.req.clone();
    let form = {
        let mut store = state.store().lock().await;

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
