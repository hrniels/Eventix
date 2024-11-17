use anyhow::anyhow;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use ical::col::CalStore;
use std::sync::{Arc, Mutex};

use crate::error::HTMLError;
use crate::extract::MultiForm;
use crate::locale::{self, Locale};
use crate::pages::Page;

use super::Request;

fn action_delete(
    page: &mut Page,
    locale: &Arc<dyn Locale + Send + Sync>,
    store: Arc<Mutex<CalStore>>,
    form: &Request,
) -> anyhow::Result<()> {
    match form.rid {
        Some(ref rid) => unimplemented!(),
        None => {
            let mut store = store.lock().unwrap();
            let item = store
                .item_by_id_mut(&form.uid)
                .ok_or_else(|| anyhow!("Unable to find item with uid {}", form.uid))?;

            item.delete_components(&form.uid);
            if item.components().len() == 0 {
                item.remove()?;
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
    let mut page = super::new_page(&form);

    action_delete(&mut page, &locale, state.store().clone(), &form)?;

    crate::monthly::index::content(
        page,
        locale,
        State(state),
        Query(crate::monthly::index::Request::default()),
    )
    .await
}
