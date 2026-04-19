// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use eventix_ical::col::{CalDir, CalFile, Occurrence};
use eventix_ical::objects::{
    CalAlarm, CalAttendee, CalCompType, CalComponent, CalPartStat, CalTodoStatus, EventLike,
};
use eventix_locale::{DateFlags, Locale};
use eventix_state::{CalendarAlarmType, EventixState, PersonalAlarms, Settings};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::Display;
use std::ops::Deref;
use std::sync::Arc;

use crate::comps::radiogroup::RadioGroupTemplate;
use crate::comps::{
    organizer::OrganizerTemplate, pagination::PaginationTemplate, partstat::PartStatTemplate,
};
use crate::extract::MultiQuery;
use crate::html::{self, filters, to_id};
use crate::pages::error::HTMLError;

const PER_PAGE: usize = 12;

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Conjunction {
    #[default]
    And,
    Or,
}

impl Display for Conjunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    keywords: String,
    page: usize,
    dirs: Vec<String>,
    conjunction: Conjunction,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            keywords: String::from(""),
            page: 1,
            dirs: Vec::new(),
            conjunction: Conjunction::default(),
        }
    }
}

impl Filter {
    pub fn url(&self) -> String {
        format!("/pages/list?{}", serde_qs::to_string(self).unwrap())
    }

    pub fn with_page(&self, page: usize) -> Self {
        Self {
            keywords: self.keywords.clone(),
            page,
            dirs: self.dirs.clone(),
            conjunction: self.conjunction,
        }
    }
}

struct ListComponent<'a> {
    dir: &'a Arc<String>,
    comp: &'a CalComponent,
    org: Option<OrganizerTemplate<'a>>,
    owner: bool,
    personal_alarms: bool,
    alarms: Option<Vec<CalAlarm>>,
    part_stat: Option<CalPartStat>,
    part_stat_btns: Option<PartStatTemplate>,
    date_range: String,
    start_display: Option<String>,
    end_display: Option<String>,
}

impl<'a> ListComponent<'a> {
    fn new<'f: 'a>(
        c: &'a CalComponent,
        file: &'f CalFile,
        locale: Arc<dyn Locale + Send + Sync>,
        settings: &'_ Settings,
        pers_alarms: &'_ PersonalAlarms,
    ) -> ListComponent<'a> {
        let ctx = file.calendar().date_context(*locale.timezone());
        let occ = Occurrence::new(
            file.directory().clone(),
            c,
            c.start().map(|d| ctx.date(d).resolved_start()),
            c.end_or_due().map(|d| ctx.date(d).resolved_end()),
            false,
        );

        let (col_settings, cal_settings) = settings.calendar(file.directory()).unwrap();
        let user_mail = col_settings.email().map(|e| e.address());
        let owner = c.is_owned_by(user_mail.as_ref());
        let part_stat = match (user_mail, owner) {
            (Some(user_mail), false) => occ.base().attendee_status(user_mail),
            _ => None,
        };
        let date_range = locale.date_range(
            c.start().cloned(),
            c.end_or_due().cloned(),
            locale.timezone(),
        );
        let start_display = c.start().map(|start| {
            if c.is_all_day() {
                locale
                    .fmt_date(
                        &ctx.date(start).start_in(locale.timezone()),
                        DateFlags::None,
                    )
                    .to_string()
            } else {
                locale
                    .fmt_datetime(
                        &ctx.date(start).start_in(locale.timezone()),
                        DateFlags::None,
                    )
                    .to_string()
            }
        });
        let end_display = c.end_or_due().map(|end| {
            if c.is_all_day() {
                locale
                    .fmt_date(&ctx.date(end).end_in(locale.timezone()), DateFlags::None)
                    .to_string()
            } else {
                locale
                    .fmt_datetime(&ctx.date(end).end_in(locale.timezone()), DateFlags::None)
                    .to_string()
            }
        });

        ListComponent {
            dir: file.directory(),
            org: c
                .organizer()
                .map(|org| OrganizerTemplate::new(locale.clone(), org)),
            comp: c,
            owner,
            alarms: pers_alarms.effective_alarms(&occ, cal_settings.alarms()),
            personal_alarms: matches!(cal_settings.alarms(), CalendarAlarmType::Personal { .. }),
            part_stat_btns: part_stat.map(|stat| {
                PartStatTemplate::new(
                    locale.clone(),
                    format!("base-{}", to_id(c.uid())),
                    stat,
                    c.uid().clone(),
                    None,
                    false,
                )
            }),
            part_stat,
            date_range,
            start_display,
            end_display,
        }
    }
}

/// Fragment-only template for the filter form and JS helpers. Loaded via AJAX into
/// `#list-shell-content` and immediately triggers a second AJAX load of the paginated results.
#[derive(Template)]
#[template(path = "pages/list.htm")]
struct ListShellTemplate<'a> {
    locale: Arc<dyn Locale + Send + Sync>,
    /// The serialized filter query string used to pre-populate the form and seed the inner
    /// content request (e.g. `"keywords=foo&page=1&dirs%5B%5D=personal&conjunction=And"`).
    filter_query: String,
    filter: Filter,
    conjunction: RadioGroupTemplate<Conjunction>,
    directories: Vec<&'a CalDir>,
}

/// Fragment-only template for the paginated list, rendered by the AJAX content endpoint.
#[derive(Template)]
#[template(path = "pages/list_results.htm")]
struct ListTemplate<'a, F: Fn(&usize) -> String> {
    locale: Arc<dyn Locale + Send + Sync>,
    comps: Vec<ListComponent<'a>>,
    pagination: PaginationTemplate<F>,
}

impl<F: Fn(&usize) -> String> ListTemplate<'_, F> {
    fn attendees_sorted(atts: &[CalAttendee]) -> Vec<&CalAttendee> {
        let mut att = atts.iter().collect::<Vec<_>>();
        att.sort_by(|a, b| match (a.common_name(), b.common_name()) {
            (Some(cn1), Some(cn2)) => cn1.cmp(cn2),
            _ => Ordering::Equal,
        });
        att
    }
}

/// Renders the list shell fragment containing the filter form, JS helpers, and the inner
/// `#list-content` placeholder. Used as the first AJAX step from the outer shell.
pub async fn content(
    State(state): State<EventixState>,
    MultiQuery(mut filter): MultiQuery<Filter>,
) -> Result<impl IntoResponse, HTMLError> {
    let st = state.lock().await;
    let locale = st.locale();

    let directories = st.store().directories().iter().collect::<Vec<_>>();
    if filter.dirs.is_empty() {
        filter.dirs = directories.iter().map(|s| s.id().deref().clone()).collect();
    }

    let filter_query = serde_qs::to_string(&filter).unwrap_or_default();

    let conjunction = RadioGroupTemplate::new(
        String::from("conjunction"),
        filter.conjunction,
        vec![
            (
                Conjunction::And,
                locale.translate("All keywords need to match").to_string(),
            ),
            (
                Conjunction::Or,
                locale.translate("Any keyword needs to match").to_string(),
            ),
        ],
    );

    let html = ListShellTemplate {
        locale,
        filter,
        conjunction,
        directories,
        filter_query,
    }
    .render()
    .context("list shell template")?;

    Ok(Html(html))
}

/// Renders only the paginated list fragment for the given filter. Used as the second AJAX step.
pub async fn content_results(
    State(state): State<EventixState>,
    MultiQuery(mut filter): MultiQuery<Filter>,
) -> Result<impl IntoResponse, HTMLError> {
    let state = state.lock().await;
    let locale = state.locale();

    let directories = state.store().directories().iter().collect::<Vec<_>>();
    if filter.dirs.is_empty() {
        filter.dirs = directories.iter().map(|s| s.id().deref().clone()).collect();
    }

    let keywords = filter.keywords.to_lowercase();
    let matches_keywords = |field: Option<&String>, kws: &String| {
        if let Some(field) = field {
            let field = field.to_lowercase();
            for kw in kws.split_whitespace() {
                if filter.conjunction == Conjunction::Or && field.contains(kw) {
                    return true;
                }
                if filter.conjunction == Conjunction::And && !field.contains(kw) {
                    return false;
                }
            }
            filter.conjunction == Conjunction::And
        } else {
            false
        }
    };

    let settings = state.settings();
    let pers_alarms = state.personal_alarms();

    let iter = || {
        state
            .store()
            .files()
            .flat_map(|file| {
                file.components()
                    .iter()
                    .filter(|c| c.rid().is_none())
                    .map(move |comp| (file, comp))
            })
            .filter(|(file, comp)| {
                if !filter.dirs.contains(file.directory()) {
                    return false;
                }
                if keywords.is_empty() {
                    return true;
                }
                if matches_keywords(comp.summary(), &keywords) {
                    return true;
                }
                if matches_keywords(comp.description(), &keywords) {
                    return true;
                }
                if matches_keywords(comp.location(), &keywords) {
                    return true;
                }
                if matches_keywords(Some(comp.uid()), &keywords) {
                    return true;
                }
                false
            })
    };
    let total = iter().count();

    let comps = iter()
        .sorted_by_key(|(_, comp)| {
            comp.last_modified()
                .or_else(|| comp.created())
                .unwrap_or_else(|| comp.stamp())
        })
        .rev()
        .skip((filter.page - 1) * PER_PAGE)
        .take(PER_PAGE)
        .map(|(file, comp)| ListComponent::new(comp, file, locale.clone(), settings, pers_alarms))
        .collect::<Vec<_>>();

    let pagination = PaginationTemplate::new(
        |page| filter.with_page(*page).url(),
        total,
        PER_PAGE,
        filter.page,
    );

    let html = ListTemplate {
        locale,
        comps,
        pagination,
    }
    .render()
    .context("list content template")?;

    Ok(Html(html))
}
