use anyhow::{anyhow, Context};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use ical::col::CalStore;
use ical::objects::{CalDate, EventLike, UpdatableEventLike};
use std::sync::Arc;
use tokio::sync::MutexGuard;

use crate::error::HTMLError;
use crate::extract::MultiForm;
use crate::locale::{self, Locale};
use crate::pages::Page;

use super::{CompAction, CompEdit};

fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    mut store: MutexGuard<'_, CalStore>,
    form: &mut CompEdit,
) -> anyhow::Result<bool> {
    let file = store.files_by_id_mut(&form.req.uid).context(format!(
        "Unable to find component with uid '{}'",
        form.req.uid
    ))?;

    let rid = if let Some(ref rid) = form.req.rid {
        Some(
            rid.parse::<CalDate>()
                .context(format!("Invalid rid date: {}", rid))?,
        )
    } else {
        None
    };

    let base = file
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
        file.component_with_mut(|c| c.uid() == &form.req.uid && c.rid() == rid.as_ref())
    {
        form.update(comp, locale);
        if rid.is_none() {
            comp.set_rrule(rrule);
        }
    } else {
        let comp = file.component_with(|c| c.uid() == &form.req.uid).unwrap();
        if !comp.is_recurrent() {
            return Err(anyhow!("Component {} is not recurrent", form.req.uid));
        }

        file.create_overwrite(&form.req.uid, rid.unwrap(), locale.timezone(), |c| {
            form.update(c, locale);
        })
        .context("Creating overwrite failed")?;
    }

    // should we move the file to a different source?
    if form.req.rid.is_none() {
        let cal = form
            .calendar
            .as_ref()
            .ok_or_else(|| anyhow!("Calendar not specified"))?;
        if *cal != **file.source() {
            let path = file.path().clone();
            let src = file.source().clone();
            store.switch_source(path, &src, &Arc::new(cal.to_string()))?;
            return Ok(true);
        }
    }

    file.save()?;
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
        let store = state.store().lock().await;
        if action_update(&mut page, &locale, store, &mut form)? {
            None
        } else {
            Some(form)
        }
    };

    super::index::content(page, locale, State(state), Query(req), form).await
}
