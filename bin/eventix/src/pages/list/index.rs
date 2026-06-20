// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use eventix_ical::col::{CalFile, Occurrence};
use eventix_ical::objects::{
    CalAlarm, CalAttendee, CalCompType, CalComponent, CalPartStat, CalTodoStatus, EventLike,
};
use eventix_locale::{DateFlags, Locale};
use eventix_state::{CalendarAlarmType, EventixState, PersonalAlarms, Settings};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::ops::Deref;
use std::sync::Arc;

use crate::comps::{
    organizer::OrganizerTemplate, pagination::PaginationTemplate, partstat::PartStatTemplate,
};
use crate::extract::MultiQuery;
use crate::html::{self, filters, to_id};
use crate::pages::error::HTMLError;

const PER_PAGE: usize = 12;
const MIN_PER_PAGE: usize = 5;
const MAX_PER_PAGE: usize = 50;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    keywords: String,
    page: usize,
    dirs: Vec<String>,
    per_page: Option<usize>,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            keywords: String::from(""),
            page: 1,
            dirs: Vec::new(),
            per_page: None,
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
            per_page: self.per_page,
        }
    }

    pub fn effective_per_page(&self) -> usize {
        self.per_page
            .unwrap_or(PER_PAGE)
            .clamp(MIN_PER_PAGE, MAX_PER_PAGE)
    }
}

#[derive(Clone, Debug)]
struct CalendarFilter<'a> {
    id: &'a str,
    name: &'a str,
    selected: bool,
}

#[derive(Debug, Eq, PartialEq)]
struct KeywordExpression {
    groups: Vec<Vec<String>>,
}

impl KeywordExpression {
    fn parse(input: &str) -> Self {
        let mut groups = Vec::new();
        let mut current = Vec::new();

        for token in input.split_whitespace() {
            if token.eq_ignore_ascii_case("OR") {
                if !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
                continue;
            }

            current.push(token.to_lowercase());
        }

        if !current.is_empty() {
            groups.push(current);
        }

        Self { groups }
    }

    fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    fn matches(&self, text: &str) -> bool {
        self.is_empty()
            || self
                .groups
                .iter()
                .any(|group| group.iter().all(|term| text.contains(term)))
    }
}

struct ListComponent<'a> {
    dir: &'a Arc<String>,
    comp: &'a CalComponent,
    org: Option<OrganizerTemplate<'a>>,
    owner: bool,
    read_only: bool,
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
        let ctx = file.calendar().date_context();
        let occ = Occurrence::new(
            file.directory().clone(),
            c,
            c.start()
                .map(|d| ctx.date(d).resolved_start(locale.timezone())),
            c.end_or_due()
                .map(|d| ctx.date(d).resolved_end(locale.timezone())),
            false,
        );

        let (col_settings, cal_settings) = settings.calendar(file.directory()).unwrap();
        let user_mail = col_settings.email().map(|e| e.address());
        let owner = c.is_owned_by(user_mail.as_ref());
        let read_only = col_settings.is_read_only();
        let part_stat = match (user_mail, owner) {
            (Some(user_mail), false) => occ.base().attendee_status(user_mail),
            _ => None,
        };
        let date_range = locale.date_range(
            c.start().cloned(),
            c.end_or_due().cloned(),
            &ctx,
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
            read_only,
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
                    read_only,
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
    filter: Filter,
    calendars: Vec<CalendarFilter<'a>>,
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

    let calendars = directories
        .into_iter()
        .map(|dir| CalendarFilter {
            id: dir.id(),
            name: dir.name(),
            selected: filter.dirs.contains(dir.id()),
        })
        .collect();

    let html = ListShellTemplate {
        locale,
        filter,
        calendars,
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

    let keywords = KeywordExpression::parse(&filter.keywords);

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

                let searchable = [
                    comp.summary().map(String::as_str),
                    comp.description().map(String::as_str),
                    comp.location().map(String::as_str),
                    Some(comp.uid().as_str()),
                ]
                .into_iter()
                .flatten()
                .join(" ")
                .to_lowercase();

                keywords.matches(&searchable)
            })
    };
    let total = iter().count();
    let per_page = filter.effective_per_page();

    let comps = iter()
        .sorted_by_key(|(_, comp)| {
            comp.last_modified()
                .or_else(|| comp.created())
                .unwrap_or_else(|| comp.stamp())
        })
        .rev()
        .skip((filter.page - 1) * per_page)
        .take(per_page)
        .map(|(file, comp)| ListComponent::new(comp, file, locale.clone(), settings, pers_alarms))
        .collect::<Vec<_>>();

    let pagination = PaginationTemplate::new(
        |page| filter.with_page(*page).url(),
        total,
        per_page,
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

#[cfg(test)]
mod tests {
    use super::KeywordExpression;

    #[test]
    fn keyword_expression_requires_all_terms_by_default() {
        let expr = KeywordExpression::parse("alpha beta");

        assert!(expr.matches("alpha gamma beta"));
        assert!(!expr.matches("alpha gamma"));
    }

    #[test]
    fn keyword_expression_splits_alternatives_on_or() {
        let expr = KeywordExpression::parse("alpha beta OR gamma");

        assert!(expr.matches("alpha something beta"));
        assert!(expr.matches("gamma"));
        assert!(!expr.matches("alpha only"));
    }

    #[test]
    fn keyword_expression_parses_or_case_insensitively() {
        let expr = KeywordExpression::parse("alpha or beta");

        assert_eq!(
            expr,
            KeywordExpression {
                groups: vec![vec![String::from("alpha")], vec![String::from("beta")]],
            }
        );
    }

    #[test]
    fn keyword_expression_ignores_empty_or_groups() {
        let expr = KeywordExpression::parse("OR alpha OR OR beta OR");

        assert_eq!(
            expr,
            KeywordExpression {
                groups: vec![vec![String::from("alpha")], vec![String::from("beta")]],
            }
        );
    }

    #[test]
    fn empty_keyword_expression_matches_anything() {
        let expr = KeywordExpression::parse("   ");

        assert!(expr.matches("anything"));
    }
}
