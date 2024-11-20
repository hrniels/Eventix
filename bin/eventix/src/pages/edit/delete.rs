use anyhow::{anyhow, Context};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use formatx::formatx;
use ical::col::CalStore;
use ical::objects::{CalDate, EventLike, UpdatableEventLike};
use std::sync::{Arc, Mutex};

use crate::error::HTMLError;
use crate::locale::{self, Locale};
use crate::pages::Page;

use super::Request;

fn action_delete(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    store: Arc<Mutex<CalStore>>,
    form: &Request,
) -> anyhow::Result<()> {
    let mut store = store.lock().unwrap();
    let item = store
        .item_by_id_mut(&form.uid)
        .ok_or_else(|| anyhow!("Unable to find item with uid {}", form.uid))?;

    match form.rid {
        Some(ref rid) => {
            let date = rid
                .parse::<CalDate>()
                .context(format!("Invalid rid date: {}", rid))?;

            let base = item
                .component_with_mut(|c| c.rid().is_none() && c.uid() == &form.uid)
                .ok_or_else(|| anyhow!("Unable to find base component with uid {}", form.uid))?;
            base.add_exdate(date.clone());
            item.save()?;

            page.add_info(
                formatx!(
                    locale.translate("Deleted occurrence on {} successfully."),
                    date.fmt_start_with_tz(locale.timezone())
                )
                .unwrap(),
            );
        }
        None => {
            item.delete_components(&form.uid);
            if item.components().is_empty() {
                item.remove()?;
            } else {
                item.save()?;
            }

            page.add_info(locale.translate("Deleted series successfully."));
        }
    }

    Ok(())
}

pub async fn handler(
    State(state): State<crate::state::State>,
    Query(form): Query<Request>,
) -> anyhow::Result<impl IntoResponse, HTMLError> {
    let locale = locale::default();
    let mut page = super::new_page();

    action_delete(&mut page, &locale, state.store().clone(), &form)?;

    crate::monthly::index::content(
        page,
        locale,
        State(state),
        Query(crate::monthly::index::Request::default()),
    )
    .await
}
