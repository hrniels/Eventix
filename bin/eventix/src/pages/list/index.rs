use anyhow::{Context, Result};
use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use ical::col::CalSource;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::ops::Deref;
use std::sync::Arc;

use ical::objects::{CalAttendee, CalCompType, CalComponent, CalDate, CalTodoStatus, EventLike};

use crate::comps::organizer::OrganizerTemplate;
use crate::comps::pagination::PaginationTemplate;
use crate::error::HTMLError;
use crate::extract::MultiQuery;
use crate::html::{self, filters};
use crate::locale::{self, DateFlags, Locale, TimeFlags};
use crate::pages::events::Events;
use crate::pages::tasks::Tasks;
use crate::pages::Page;

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
        format!("/list?{}", serde_qs::to_string(self).unwrap())
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
    org: Option<OrganizerTemplate<'a>>,
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

impl<'a, F: Fn(&usize) -> String> ListTemplate<'a, F> {
    fn attendees_sorted(atts: &[CalAttendee]) -> Vec<&CalAttendee> {
        let mut att = atts.iter().collect::<Vec<_>>();
        att.sort_by(|a, b| match (a.common_name(), b.common_name()) {
            (Some(cn1), Some(cn2)) => cn1.cmp(cn2),
            _ => Ordering::Equal,
        });
        att
    }
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
    let matches_keywords = |field: Option<&String>, kws: &String| {
        if let Some(field) = field {
            let field = field.to_lowercase();
            for kw in kws.split_whitespace() {
                if field.contains(kw) {
                    return true;
                }
            }
        }
        false
    };

    let iter = || {
        store
            .items()
            .flat_map(|i| {
                i.components()
                    .into_iter()
                    .filter(|c| c.rid().is_none())
                    .map(|c| ListComponent {
                        source: i.source(),
                        org: c
                            .organizer()
                            .map(|org| OrganizerTemplate::new(locale.clone(), org)),
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
                if matches_keywords(l.comp.summary(), &filter.keywords) {
                    return true;
                }
                if matches_keywords(l.comp.description(), &filter.keywords) {
                    return true;
                }
                if matches_keywords(l.comp.location(), &filter.keywords) {
                    return true;
                }
                if matches_keywords(Some(l.comp.uid()), &filter.keywords) {
                    return true;
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
