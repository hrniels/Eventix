use anyhow::{Context, Result};
use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use ical::col::{CalDir, Occurrence};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::ops::Deref;
use std::sync::Arc;

use ical::objects::{
    CalAlarm, CalAttendee, CalCompType, CalComponent, CalDate, CalTodoStatus, EventLike,
};

use crate::comps::organizer::OrganizerTemplate;
use crate::comps::pagination::PaginationTemplate;
use crate::comps::partstat::PartStatTemplate;
use crate::error::HTMLError;
use crate::extract::MultiQuery;
use crate::html::{self, filters};
use crate::locale::{self, DateFlags, Locale, TimeFlags};
use crate::pages::Page;
use crate::pages::events::Events;
use crate::pages::tasks::Tasks;
use crate::state::{CalendarAlarmType, EventixState};
use crate::util;

const PER_PAGE: usize = 15;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    keywords: String,
    page: usize,
    dirs: Vec<String>,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            keywords: String::from(""),
            page: 1,
            dirs: Vec::new(),
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
            dirs: self.dirs.clone(),
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
    partstat: Option<PartStatTemplate>,
}

#[derive(Template)]
#[template(path = "pages/list.htm")]
struct ListTemplate<'a, F: Fn(&usize) -> String> {
    page: Page,
    locale: Arc<dyn Locale + Send + Sync>,
    filter: Filter,
    directories: Vec<&'a CalDir>,
    comps: Vec<ListComponent<'a>>,
    pagination: PaginationTemplate<F>,
    events: Events<'a>,
    tasks: Tasks<'a>,
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

pub async fn handler(
    State(state): State<EventixState>,
    MultiQuery(mut filter): MultiQuery<Filter>,
) -> Result<impl IntoResponse, HTMLError> {
    let page = super::new_page(&state, &filter).await;
    let locale = locale::default();

    let state = state.lock().await;

    let directories = state.store().directories().iter().collect::<Vec<_>>();
    if filter.dirs.is_empty() {
        filter.dirs = directories.iter().map(|s| s.id().deref().clone()).collect();
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

    let settings = state.settings();
    let pers_alarms = state.personal_alarms();

    let iter = || {
        state
            .store()
            .files()
            .flat_map(|i| {
                i.components()
                    .iter()
                    .filter(|c| c.rid().is_none())
                    .map(|c| {
                        let cal_settings = settings.calendar(i.directory()).unwrap();
                        let occ = Occurrence::new(
                            i.directory().clone(),
                            c,
                            c.start().map(|d| d.as_start_with_tz(locale.timezone())),
                            c.end_or_due()
                                .map(|d| d.as_start_with_tz(locale.timezone())),
                            false,
                        );
                        let owner = util::user_is_event_owner(i.directory(), &state, c);
                        let user_mail = cal_settings.email().map(|e| e.address());
                        let part_stat = match (user_mail, owner) {
                            (Some(user_mail), false) => occ.base().attendee_status(user_mail),
                            _ => None,
                        };
                        ListComponent {
                            dir: i.directory(),
                            org: c
                                .organizer()
                                .map(|org| OrganizerTemplate::new(locale.clone(), org)),
                            comp: c,
                            owner,
                            alarms: pers_alarms.effective_alarms(&occ, cal_settings.alarms()),
                            personal_alarms: matches!(
                                cal_settings.alarms(),
                                CalendarAlarmType::Personal { .. }
                            ),
                            partstat: part_stat.map(|stat| {
                                PartStatTemplate::new(
                                    locale.clone(),
                                    format!("base-{}", c.uid()),
                                    stat,
                                    c.uid().clone(),
                                    None,
                                    false,
                                )
                            }),
                        }
                    })
            })
            .filter(|l| {
                if !filter.dirs.contains(l.dir) {
                    return false;
                }
                if keywords.is_empty() {
                    return true;
                }
                if matches_keywords(l.comp.summary(), &keywords) {
                    return true;
                }
                if matches_keywords(l.comp.description(), &keywords) {
                    return true;
                }
                if matches_keywords(l.comp.location(), &keywords) {
                    return true;
                }
                if matches_keywords(Some(l.comp.uid()), &keywords) {
                    return true;
                }
                false
            })
    };
    let total = iter().count();

    let comps = iter()
        .sorted_by_key(|c| (c.comp.last_modified(), c.comp.created(), c.comp.stamp()))
        .rev()
        .skip((filter.page - 1) * PER_PAGE)
        .take(PER_PAGE)
        .collect::<Vec<_>>();

    let events = Events::new(&state, &locale);
    let tasks = Tasks::new(&state, &locale);

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
        directories,
        comps,
        pagination,
        events,
        tasks,
    }
    .render()
    .context("search template")?;

    Ok(Html(html))
}
