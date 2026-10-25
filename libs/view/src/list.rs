// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Base-component browsing, independent of occurrence expansion and calendar visibility.

use std::sync::Arc;

use eventix_ical::{
    col::{CalFile, Occurrence},
    objects::{CalCompType, CalComponent, CalDate, CalPartStat, CalTodoStatus, EventLike},
};
use eventix_locale::Locale;
use eventix_state::State;
use eventix_state::actions::items::ItemId;

/// Applied list selection. An empty calendar selection means all calendars.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub keywords: String,
    pub calendars: Vec<String>,
}

/// Case-insensitive AND groups separated by the standalone word `OR`.
#[derive(Debug, PartialEq, Eq)]
pub struct KeywordExpression(Vec<Vec<String>>);

impl KeywordExpression {
    /// Parses whitespace-separated terms and ignores empty alternatives.
    pub fn parse(input: &str) -> Self {
        let mut groups = Vec::new();
        let mut current = Vec::new();
        for token in input.split_whitespace() {
            if token.eq_ignore_ascii_case("OR") {
                if !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
            } else {
                current.push(token.to_lowercase());
            }
        }
        if !current.is_empty() {
            groups.push(current);
        }
        Self(groups)
    }

    /// Matches text using case-insensitive substring comparisons.
    pub fn matches(&self, text: &str) -> bool {
        let text = text.to_lowercase();
        self.0.is_empty()
            || self
                .0
                .iter()
                .any(|group| group.iter().all(|term| text.contains(term)))
    }
}

/// A calendar offered by the list's independent filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calendar {
    pub id: String,
    pub name: String,
    pub background: String,
}

/// Compact presentation of a base event or task, never an expanded occurrence.
#[derive(Clone, Debug)]
pub struct Row {
    pub calendar: String,
    pub calendar_name: String,
    pub uid: String,
    pub summary: String,
    pub background: String,
    pub task: bool,
    pub completed: bool,
    pub recurrent: bool,
    pub alarm: bool,
    pub response: Option<CalPartStat>,
    pub date_range: String,
    pub recurrence: String,
}

/// A lightweight ordered index of matching base components.
///
/// Row presentation data is loaded separately, only when requested by the view.
pub struct Snapshot {
    pub filter: Filter,
    pub calendars: Vec<Calendar>,
    pub items: Vec<ItemId>,
    pub locale: Arc<dyn Locale + Send + Sync>,
}

impl Snapshot {
    /// Loads base components, including past events and completed or undated tasks.
    pub fn load(state: &State, filter: &Filter) -> Self {
        let locale = state.locale();
        let calendars: Vec<_> = state
            .store()
            .directories()
            .iter()
            .map(|dir| Calendar {
                id: dir.id().to_string(),
                name: dir.name().to_string(),
                background: state
                    .settings()
                    .calendar(dir.id())
                    .map(|(_, settings)| settings.bgcolor().clone())
                    .unwrap_or_default(),
            })
            .collect();
        let mut filter = filter.clone();
        filter
            .calendars
            .retain(|id| calendars.iter().any(|calendar| &calendar.id == id));
        let keywords = KeywordExpression::parse(&filter.keywords);
        let mut components: Vec<_> = state
            .store()
            .files()
            .flat_map(|file| {
                file.components()
                    .iter()
                    .filter(|comp| comp.rid().is_none())
                    .map(move |comp| (file, comp))
            })
            .filter(|(file, comp)| {
                (filter.calendars.is_empty()
                    || filter.calendars.iter().any(|id| id == &**file.directory()))
                    && keywords.matches(
                        &[
                            comp.summary().map(String::as_str),
                            comp.description().map(String::as_str),
                            comp.location().map(String::as_str),
                            Some(comp.uid().as_str()),
                        ]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" "),
                    )
            })
            .collect();
        components.sort_by(|(af, a), (bf, b)| {
            modified(b)
                .cmp(modified(a))
                .then_with(|| (af.directory(), a.uid()).cmp(&(bf.directory(), b.uid())))
        });
        let items = components
            .into_iter()
            .map(|(file, comp)| ItemId {
                calendar: file.directory().to_string(),
                uid: comp.uid().clone(),
                recurrence: None,
            })
            .collect();
        Self {
            filter,
            calendars,
            items,
            locale,
        }
    }
}

fn modified(comp: &CalComponent) -> &CalDate {
    comp.last_modified()
        .or_else(|| comp.created())
        .unwrap_or_else(|| comp.stamp())
}

impl Row {
    /// Loads presentation data for a single base component, or returns none if unavailable.
    pub fn load(state: &State, item: &ItemId) -> Option<Self> {
        if item.recurrence.is_some() {
            return None;
        }
        let dir = state.store().directory(&Arc::new(item.calendar.clone()))?;
        let file = dir.file_by_id(&item.uid)?;
        let comp = file.component_with(|comp| comp.uid() == &item.uid && comp.rid().is_none())?;
        Self::from_component(state, file, comp, &*state.locale())
    }

    fn from_component(
        state: &State,
        file: &CalFile,
        comp: &CalComponent,
        locale: &dyn Locale,
    ) -> Option<Self> {
        let (collection, settings) = state.settings().calendar(file.directory())?;
        let ctx = file.calendar().date_context();
        let occ = Occurrence::new(
            file.directory().clone(),
            comp,
            comp.start()
                .map(|date| ctx.date(date).resolved_start(locale.timezone())),
            comp.end_or_due()
                .map(|date| ctx.date(date).resolved_end(locale.timezone())),
            false,
        );
        let email = collection.email().map(|email| email.address());
        let response = email
            .as_ref()
            .filter(|email| !comp.is_owned_by(Some(email)))
            .map(|email| {
                comp.attendee_status(email)
                    .unwrap_or(CalPartStat::NeedsAction)
            });

        Some(Self {
            calendar: file.directory().to_string(),
            uid: comp.uid().to_string(),
            calendar_name: settings.name().clone(),
            summary: comp.summary().cloned().unwrap_or_default(),
            background: settings.bgcolor().clone(),
            task: comp.ctype() == CalCompType::Todo,
            completed: comp
                .as_todo()
                .is_some_and(|todo| todo.status() == Some(CalTodoStatus::Completed)),
            recurrent: comp.is_recurrent(),
            alarm: state.personal_alarms().has_alarms(&occ, settings.alarms()),
            response,
            // Locale's web-oriented range formatter encodes its separator as an HTML entity.
            date_range: locale
                .date_range(
                    comp.start().cloned(),
                    comp.end_or_due().cloned(),
                    &ctx,
                    locale.timezone(),
                )
                .replace("&#x2012;", "‒"),
            recurrence: comp
                .rrule()
                .map(|rule| rule.human_interval(locale).to_string())
                .unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eventix_ical::col::{CalDir, CalFile};
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};

    use super::{Filter, KeywordExpression, Row, Snapshot};

    fn state(components: &[&str]) -> (tempfile::TempDir, State) {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let mut state =
            State::new_with_timezone(Arc::new(xdg), Some(chrono_tz::Europe::Berlin)).unwrap();
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: tmp.path().display().to_string(),
        });
        for id in ["work", "home"] {
            let mut settings = CalendarSettings::default();
            settings.set_enabled(true);
            settings.set_name(id.into());
            collection.all_calendars_mut().insert(id.into(), settings);
            let id = Arc::new(id.to_string());
            let mut dir = CalDir::new_empty(id.clone(), tmp.path().into(), id.to_string(), false);
            for (index, component) in components.iter().enumerate() {
                let calendar =
                    format!("BEGIN:VCALENDAR\nVERSION:2.0\n{component}\nEND:VCALENDAR\n")
                        .parse()
                        .unwrap();
                dir.add_file(CalFile::new(
                    id.clone(),
                    tmp.path().join(format!("{id}-{index}.ics")),
                    calendar,
                ))
                .unwrap();
            }
            state.store_mut().add(dir);
        }
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        (tmp, state)
    }

    #[test]
    fn base_rows_include_completed_undated_and_past_items_without_expanding_series() {
        let (_tmp, mut state) = state(&[
            "BEGIN:VEVENT\nUID:past\nDTSTART;VALUE=DATE:20200101\nDTEND;VALUE=DATE:20200103\nSUMMARY:Old event\nEND:VEVENT",
            "BEGIN:VTODO\nUID:done\nSUMMARY:Finished\nSTATUS:COMPLETED\nEND:VTODO",
            "BEGIN:VEVENT\nUID:series\nDTSTART:20261005T090000Z\nDTEND:20261005T100000Z\nRRULE:FREQ=DAILY;COUNT=3\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT10M\nDESCRIPTION:Reminder\nEND:VALARM\nEND:VEVENT\nBEGIN:VEVENT\nUID:series\nRECURRENCE-ID:20261006T090000Z\nDTSTART:20261006T100000Z\nDTEND:20261006T110000Z\nEND:VEVENT",
        ]);
        state.misc_mut().toggle_calendar(&"work".to_string());
        let snapshot = Snapshot::load(
            &state,
            &Filter {
                calendars: vec!["work".into()],
                ..Default::default()
            },
        );
        assert_eq!(snapshot.items.len(), 3);
        assert_eq!(snapshot.calendars.len(), 2);
        let rows: Vec<_> = snapshot
            .items
            .iter()
            .filter_map(|item| Row::load(&state, item))
            .collect();
        let done = rows.iter().find(|row| row.uid == "done").unwrap();
        assert!(done.task && done.completed);
        assert_eq!(done.date_range, "-");
        let past = rows.iter().find(|row| row.uid == "past").unwrap();
        assert!(past.date_range.contains('‒'));
        assert!(!past.date_range.contains("&#"));
        let series = rows.iter().find(|row| row.uid == "series").unwrap();
        assert!(series.recurrent && series.alarm);
        assert!(!series.recurrence.is_empty());
        assert!(
            series.date_range.contains("11:00"),
            "dates use the profile timezone"
        );
    }

    #[test]
    fn search_checks_all_fields_and_combines_terms_across_fields() {
        let (_tmp, state) = state(&[
            "BEGIN:VTODO\nUID:uid-needle\nSUMMARY:Alpha\nDESCRIPTION:Beta\nLOCATION:Office\nEND:VTODO",
            "BEGIN:VTODO\nUID:other\nSUMMARY:Gamma\nEND:VTODO",
        ]);
        for keywords in ["ALPHA beta", "office", "uid-needle", "missing OR gamma"] {
            let snapshot = Snapshot::load(
                &state,
                &Filter {
                    keywords: keywords.into(),
                    calendars: vec!["home".into()],
                },
            );
            assert_eq!(snapshot.items.len(), 1, "{keywords}");
        }
        assert_eq!(
            Snapshot::load(
                &state,
                &Filter {
                    keywords: "alpha missing".into(),
                    ..Default::default()
                }
            )
            .items
            .len(),
            0
        );
    }

    #[test]
    fn index_includes_all_matches_in_modified_created_and_stamp_order() {
        let components: Vec<_> = (1..=14).map(|index| {
            format!("BEGIN:VTODO\nUID:item-{index:02}\nDTSTAMP:20260101T000000Z\nCREATED:202601{index:02}T000000Z\nEND:VTODO")
        }).collect();
        let refs: Vec<_> = components.iter().map(String::as_str).collect();
        let (_tmp, state) = state(&refs);
        let mut filter = Filter {
            calendars: vec!["home".into()],
            ..Default::default()
        };
        let snapshot = Snapshot::load(&state, &filter);
        assert_eq!(snapshot.items.len(), 14);
        assert_eq!(snapshot.items[0].uid, "item-14");
        assert_eq!(snapshot.items[13].uid, "item-01");
        let last = Row::load(&state, &snapshot.items[13]).unwrap();
        assert_eq!(last.uid, "item-01");
        filter.keywords = "not-found".into();
        let empty = Snapshot::load(&state, &filter);
        assert!(empty.items.is_empty());

        let (_tmp, state) = self::state(&[
            "BEGIN:VTODO\nUID:modified\nDTSTAMP:20260101T000000Z\nCREATED:20260102T000000Z\nLAST-MODIFIED:20260104T000000Z\nEND:VTODO",
            "BEGIN:VTODO\nUID:created\nDTSTAMP:20260101T000000Z\nCREATED:20260103T000000Z\nEND:VTODO",
            "BEGIN:VTODO\nUID:stamp\nDTSTAMP:20260102T000000Z\nEND:VTODO",
        ]);
        let snapshot = Snapshot::load(
            &state,
            &Filter {
                calendars: vec!["home".into()],
                ..Default::default()
            },
        );
        assert_eq!(
            snapshot
                .items
                .iter()
                .map(|row| row.uid.as_str())
                .collect::<Vec<_>>(),
            ["modified", "created", "stamp"]
        );
    }

    #[test]
    fn keyword_groups_are_case_insensitive_and_ignore_empty_alternatives() {
        let expression = KeywordExpression::parse("OR Alpha beta or OR gamma OR");
        assert!(expression.matches("BETA and ALPHA"));
        assert!(expression.matches("GAMMA"));
        assert!(!expression.matches("alpha only"));
        assert!(KeywordExpression::parse(" OR  ").matches("anything"));
    }

    #[test]
    fn row_loading_rejects_missing_components_and_recurrence_exceptions() {
        let (_tmp, state) = state(&["BEGIN:VTODO\nUID:task\nSUMMARY:Task\nEND:VTODO"]);
        let snapshot = Snapshot::load(&state, &Filter::default());
        let mut item = snapshot.items[0].clone();
        item.calendar = "missing".into();
        assert!(Row::load(&state, &item).is_none());
        item = snapshot.items[0].clone();
        item.uid = "missing".into();
        assert!(Row::load(&state, &item).is_none());
        item = snapshot.items[0].clone();
        item.recurrence = Some("TU2026-10-25T09:00:00".parse().unwrap());
        assert!(Row::load(&state, &item).is_none());
    }
}
