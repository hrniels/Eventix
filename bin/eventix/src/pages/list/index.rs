use anyhow::{Context, Result};
use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use ical::col::CalSource;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::sync::Arc;

use ical::objects::{CalCompType, CalComponent, CalDate, CalTodoStatus, EventLike};

use crate::comps::pagination::PaginationTemplate;
use crate::error::HTMLError;
use crate::extract::MultiQuery;
use crate::html::filters;
use crate::locale::{self, Locale};
use crate::pages::events::Events;
use crate::pages::tasks::Tasks;
use crate::pages::Page;

use super::path;

const PER_PAGE: usize = 15;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    keywords: String,
    page: usize,
    sources: Vec<String>,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            keywords: String::from(""),
            page: 1,
            sources: Vec::new(),
        }
    }
}

impl Filter {
    pub fn url(&self) -> String {
        format!("{}?{}", path(), serde_qs::to_string(self).unwrap())
    }

    pub fn with_page(&self, page: usize) -> Self {
        Self {
            keywords: self.keywords.clone(),
            page,
            sources: self.sources.clone(),
        }
    }
}

struct ListComponent<'a> {
    source: &'a Arc<String>,
    comp: &'a CalComponent,
}

#[derive(Template)]
#[template(path = "pages/list.htm")]
struct ListTemplate<'a, F: Fn(&usize) -> String> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    filter: Filter,
    sources: Vec<&'a CalSource>,
    comps: Vec<ListComponent<'a>>,
    pagination: PaginationTemplate<F>,
    events: Events<'a>,
    tasks: Tasks<'a>,
}

pub async fn handler(
    State(state): State<crate::state::State>,
    MultiQuery(mut filter): MultiQuery<Filter>,
) -> Result<impl IntoResponse, HTMLError> {
    let page = super::new_page(&state, &filter).await;
    let locale = locale::default();

    let (store, disabled) = state.acquire_store_and_disabled().await;

    let sources = store.sources().iter().collect::<Vec<_>>();
    if filter.sources.is_empty() {
        filter.sources = sources.iter().map(|s| s.id().deref().clone()).collect();
    }

    let keywords = filter.keywords.to_lowercase();
    let matches_keywords = |field: &String, kws: &String| {
        for kw in kws.split_whitespace() {
            if field.contains(kw) {
                return true;
            }
        }
        false
    };

    let iter = || {
        store
            .items()
            .flat_map(|i| {
                i.components().into_iter().map(|c| ListComponent {
                    source: i.source(),
                    comp: c,
                })
            })
            .filter(|l| {
                if !filter.sources.contains(l.source) {
                    return false;
                }
                if keywords.is_empty() {
                    return true;
                }
                if let Some(summary) = l.comp.summary() {
                    if matches_keywords(&summary.to_lowercase(), &filter.keywords) {
                        return true;
                    }
                }
                if let Some(desc) = l.comp.description() {
                    if matches_keywords(&desc.to_lowercase(), &filter.keywords) {
                        return true;
                    }
                }
                if let Some(loc) = l.comp.location() {
                    if matches_keywords(&loc.to_lowercase(), &filter.keywords) {
                        return true;
                    }
                }
                false
            })
    };
    let total = iter().count();

    let comps = iter()
        .sorted_by_key(|c| c.comp.created())
        .rev()
        .skip((filter.page - 1) * PER_PAGE)
        .take(PER_PAGE)
        .collect::<Vec<_>>();

    let events = Events::new(&store, &disabled, &locale);
    let tasks = Tasks::new(&store, &disabled, &locale);

    let filter_clone = filter.clone();
    let pagination = PaginationTemplate::new(
        |page| filter_clone.with_page(*page).url(),
        total,
        PER_PAGE,
        filter.page,
    );

    let html = ListTemplate {
        page,
        locale,
        filter,
        sources,
        comps,
        pagination,
        events,
        tasks,
    }
    .render()
    .context("search template")?;

    Ok(Html(html))
}
