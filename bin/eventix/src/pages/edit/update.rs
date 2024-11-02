use anyhow::Context;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Form;
use ical::col::CalItem;
use ical::objects::{CalDate, UpdatableEventLike};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::HTMLError;
use crate::locale::{self, Locale};
use crate::pages::Page;

use super::Request;

#[derive(Debug, Deserialize)]
pub struct Update {
    base: Request,
    summary: String,
}

fn action_update(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    item: &mut CalItem,
    form: &Update,
) -> anyhow::Result<()> {
    if form.summary.is_empty() {
        page.add_error(locale.translate("Summary cannot be empty."));
        return Ok(());
    }

    let base = item
        .base_with_mut(|_| true)
        .context("Unable to find base component")?;

    base.set_summary(form.summary.clone());
    base.set_last_modified(CalDate::now());
    base.set_stamp(CalDate::now());

    item.save()
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Form(form): Form<Update>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let mut page = super::new_page(&form.base);

    {
        let mut store = state.store().lock().unwrap();

        let item = store.item_by_id_mut(&form.base.uid).context(format!(
            "Unable to find component with uid '{}'",
            form.base.uid
        ))?;

        action_update(&mut page, &locale, item, &form)?;
    }

    super::index::content(page, locale, State(state), Query(form.base)).await
}
