// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Base-component browsing and occurrence expansion, independent of calendar visibility.

pub mod occurrences;

use std::sync::Arc;

use eventix_ical::{
    col::{CalFile, Occurrence},
    objects::{CalCompType, CalComponent, CalDate, CalPartStat, CalTodoStatus, EventLike},
};
use eventix_locale::Locale;
use eventix_state::actions::items::ItemId;
use eventix_state::{CalendarAlarmType, State};

pub use crate::item_actions::Action;

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
    pub actions: Vec<Action>,
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
        let owner = comp.is_owned_by(email.as_ref());
        let part_stat = email
            .as_ref()
            .filter(|_| !owner)
            .and_then(|email| comp.attendee_status(email));
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
            actions: actions(
                comp,
                owner,
                collection.is_read_only(),
                matches!(settings.alarms(), CalendarAlarmType::Personal { .. }),
                part_stat,
            ),
            date_range: locale.date_range(
                comp.start().cloned(),
                comp.end_or_due().cloned(),
                &ctx,
                locale.timezone(),
            ),
            recurrence: comp
                .rrule()
                .map(|rule| rule.human_interval(locale).to_string())
                .unwrap_or_default(),
        })
    }
}

fn actions(
    comp: &CalComponent,
    owner: bool,
    read_only: bool,
    personal_alarms: bool,
    part_stat: Option<CalPartStat>,
) -> Vec<Action> {
    let mut actions = Vec::new();
    if let Some(todo) = comp.as_todo()
        && !read_only
        && !comp.is_recurrent()
        && todo.status() != Some(CalTodoStatus::Completed)
    {
        actions.push(Action::Complete);
        if owner && comp.end_or_due().is_some() {
            actions.push(Action::Postpone);
        }
    }
    if !read_only {
        actions.push(Action::Delete);
    }
    if owner {
        if !read_only {
            actions.push(Action::Edit);
        }
    } else {
        if let Some(status) = part_stat
            && (!read_only
                || matches!(
                    status,
                    CalPartStat::Accepted | CalPartStat::Tentative | CalPartStat::Declined
                ))
        {
            actions.push(Action::Respond { status, read_only });
        }
        if personal_alarms {
            actions.push(Action::Alarms);
        }
    }
    actions.push(Action::CopyUid);
    actions
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eventix_ical::{
        col::{CalDir, CalFile},
        objects::{CalPartStat, UpdatableEventLike},
    };
    use eventix_state::actions::items::ItemId;
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};

    use super::{Action, Filter, KeywordExpression, Row, Snapshot, actions};

    fn available_actions(
        component: &str,
        owner: bool,
        read_only: bool,
        personal_alarms: bool,
        response: Option<CalPartStat>,
    ) -> Vec<Action> {
        let calendar: eventix_ical::objects::Calendar =
            format!("BEGIN:VCALENDAR\nVERSION:2.0\n{component}\nEND:VCALENDAR\n")
                .parse()
                .unwrap();
        actions(
            &calendar.components()[0],
            owner,
            read_only,
            personal_alarms,
            response,
        )
    }

    #[test]
    fn task_actions_require_writable_incomplete_nonrecurrent_tasks() {
        for (fields, expected) in [
            ("", vec![Action::Complete]),
            (
                "DUE;VALUE=DATE:20261025\n",
                vec![Action::Complete, Action::Postpone],
            ),
            ("STATUS:COMPLETED\n", vec![]),
            (
                "DTSTART;VALUE=DATE:20261025\nDUE;VALUE=DATE:20261026\nRRULE:FREQ=DAILY\n",
                vec![],
            ),
        ] {
            let component = format!("BEGIN:VTODO\nUID:task\n{fields}END:VTODO");
            let mut expected = expected;
            expected.extend([Action::Delete, Action::Edit, Action::CopyUid]);
            assert_eq!(
                available_actions(&component, true, false, false, None),
                expected,
                "{fields}"
            );
            assert_eq!(
                available_actions(&component, true, true, false, None),
                [Action::CopyUid]
            );
        }
        let assigned = "BEGIN:VTODO\nUID:task\nDUE;VALUE=DATE:20261025\nEND:VTODO";
        assert_eq!(
            available_actions(
                assigned,
                false,
                false,
                false,
                Some(CalPartStat::NeedsAction)
            ),
            [
                Action::Complete,
                Action::Delete,
                Action::Respond {
                    status: CalPartStat::NeedsAction,
                    read_only: false,
                },
                Action::CopyUid,
            ]
        );
    }

    #[test]
    fn invitation_actions_preserve_response_status_and_personal_alarm_permissions() {
        let event = "BEGIN:VEVENT\nUID:event\nDTSTART;VALUE=DATE:20261025\nEND:VEVENT";
        assert_eq!(
            available_actions(event, true, false, true, None),
            [Action::Delete, Action::Edit, Action::CopyUid]
        );
        for read_only in [false, true] {
            for status in [
                CalPartStat::NeedsAction,
                CalPartStat::Accepted,
                CalPartStat::Tentative,
                CalPartStat::Declined,
            ] {
                let mut expected = Vec::new();
                if !read_only {
                    expected.push(Action::Delete);
                }
                if !read_only || status != CalPartStat::NeedsAction {
                    expected.push(Action::Respond { status, read_only });
                }
                expected.extend([Action::Alarms, Action::CopyUid]);
                assert_eq!(
                    available_actions(event, false, read_only, true, Some(status)),
                    expected
                );
            }
            assert_eq!(
                available_actions(event, false, read_only, false, None),
                if read_only {
                    vec![Action::CopyUid]
                } else {
                    vec![Action::Delete, Action::CopyUid]
                }
            );
        }
    }

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
    fn occurrence_pages_include_boundaries_exclusions_and_equal_start_overrides_once() {
        use crate::list::occurrences::{Cursor, Direction, Page};
        use chrono::TimeZone;
        let (_tmp, state) = state(&["BEGIN:VEVENT\nUID:series\nDTSTART;VALUE=DATE:20261015\n\
             RRULE:FREQ=DAILY;COUNT=20\nEXDATE;VALUE=DATE:20261024\nSUMMARY:Base\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:series\nRECURRENCE-ID;VALUE=DATE:20261026\n\
             DTSTART;VALUE=DATE:20261025\nSUMMARY:Moved\nEND:VEVENT"]);
        let item = ItemId {
            calendar: "work".into(),
            uid: "series".into(),
            recurrence: None,
        };
        let cursor = Cursor {
            start: chrono_tz::Europe::Berlin
                .with_ymd_and_hms(2026, 10, 25, 0, 0, 0)
                .unwrap(),
            recurrence: String::new(),
        };
        let earlier = Page::load(&state, &item, &cursor, Direction::Earlier, 5);
        assert_eq!(earlier.rows.len(), 5);
        assert!(earlier.more);
        assert!(earlier.rows.last().unwrap().excluded);
        assert_eq!(earlier.rows.last().unwrap().item.calendar, "work");
        let oldest = Page::load(
            &state,
            &item,
            &earlier.rows[0].cursor,
            Direction::Earlier,
            5,
        );
        assert_eq!(oldest.rows.len(), 5);
        assert!(!oldest.more);
        let first = Page::load(&state, &item, &cursor, Direction::From, 1);
        assert_eq!(first.rows.len(), 1);
        assert_eq!(first.rows[0].cursor.start, cursor.start);
        assert!(first.more);
        let second = Page::load(&state, &item, &first.rows[0].cursor, Direction::Later, 1);
        assert_eq!(second.rows[0].cursor.start, cursor.start);
        assert_ne!(first.rows[0].item, second.rows[0].item);
        assert!(second.rows[0].overwritten);
        assert_eq!(second.rows[0].summary, "Moved");
        let mut all = oldest.rows;
        all.extend(earlier.rows);
        let mut boundary = cursor;
        let mut dir = Direction::From;
        loop {
            let page = Page::load(&state, &item, &boundary, dir, 3);
            if let Some(last) = page.rows.last() {
                boundary = last.cursor.clone();
            }
            all.extend(page.rows);
            if !page.more {
                break;
            }
            dir = Direction::Later;
        }
        assert_eq!(all.len(), 20);
        assert!(all.windows(2).all(|pair| pair[0].cursor < pair[1].cursor));
        let ids = all
            .iter()
            .map(|row| row.item.recurrence.as_ref().unwrap().to_string())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), 20);
    }

    #[test]
    fn occurrence_pages_support_tasks_without_due_dates_and_respect_calendar_identity() {
        use crate::list::occurrences::{Cursor, Direction, Page};
        use chrono::TimeZone;
        let (_tmp, state) = state(&["BEGIN:VTODO\nUID:task\nDTSTART;VALUE=DATE:20261025\n\
             RRULE:FREQ=DAILY;COUNT=2\nSTATUS:COMPLETED\nEND:VTODO"]);
        let mut item = ItemId {
            calendar: "home".into(),
            uid: "task".into(),
            recurrence: None,
        };
        let cursor = Cursor {
            start: chrono_tz::Europe::Berlin
                .with_ymd_and_hms(2026, 10, 25, 0, 0, 0)
                .unwrap(),
            recurrence: String::new(),
        };
        let page = Page::load(&state, &item, &cursor, Direction::From, 5);
        assert_eq!(page.rows.len(), 2);
        assert!(!page.more);
        for row in &page.rows {
            assert_eq!(row.item.calendar, "home");
            assert!(row.task && row.completed);
            assert!(!row.details.actions.complete);
            assert!(!row.details.actions.postpone);
            assert!(!row.details.date.is_empty());
        }
        item.calendar = "missing".into();
        assert!(
            Page::load(&state, &item, &cursor, Direction::From, 5)
                .rows
                .is_empty()
        );
    }

    #[test]
    fn occurrence_markers_inherit_calendar_colors_and_use_overridden_attendee_responses() {
        use crate::{
            details::Response,
            list::occurrences::{Cursor, Direction, Page},
        };
        use chrono::TimeZone;
        let (_tmp, mut state) = state(&[
            "BEGIN:VEVENT\nUID:invitation\nDTSTART;VALUE=DATE:20261025\n\
             RRULE:FREQ=DAILY;COUNT=4\nORGANIZER:mailto:owner@example.org\n\
             ATTENDEE;PARTSTAT=ACCEPTED:mailto:guest@example.org\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:invitation\nRECURRENCE-ID;VALUE=DATE:20261026\n\
             DTSTART;VALUE=DATE:20261026\nATTENDEE;PARTSTAT=TENTATIVE:mailto:guest@example.org\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:invitation\nRECURRENCE-ID;VALUE=DATE:20261027\n\
             DTSTART;VALUE=DATE:20261027\nATTENDEE;PARTSTAT=DECLINED:mailto:guest@example.org\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:invitation\nRECURRENCE-ID;VALUE=DATE:20261028\n\
             DTSTART;VALUE=DATE:20261028\nATTENDEE:mailto:guest@example.org\nEND:VEVENT",
        ]);
        let collection = state
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap();
        collection
            .all_calendars_mut()
            .get_mut("work")
            .unwrap()
            .set_bgcolor("#803366".into());
        let syncer = |mail: &str| SyncerType::VDirSyncer {
            email: eventix_state::EmailAccount::new("User".into(), mail.into()),
            url: "https://example.org".into(),
            read_only: false,
            username: None,
            password: None,
            time_span: Default::default(),
        };
        collection.set_syncer(syncer("guest@example.org"));
        let item = ItemId {
            calendar: "work".into(),
            uid: "invitation".into(),
            recurrence: None,
        };
        let cursor = Cursor {
            start: chrono_tz::Europe::Berlin
                .with_ymd_and_hms(2026, 10, 25, 0, 0, 0)
                .unwrap(),
            recurrence: String::new(),
        };
        let page = Page::load(&state, &item, &cursor, Direction::From, 5);
        assert_eq!(
            page.rows.iter().map(|row| row.response).collect::<Vec<_>>(),
            [
                Response::Accepted,
                Response::Tentative,
                Response::Declined,
                Response::Pending
            ]
            .map(Some)
        );
        let base = Row::load(&state, &item).unwrap();
        for row in page.rows {
            assert_eq!(row.background, base.background);
            assert_eq!(row.calendar_name, base.calendar_name);
        }
        state
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .set_syncer(syncer("owner@example.org"));
        assert!(
            Page::load(&state, &item, &cursor, Direction::From, 5)
                .rows
                .iter()
                .all(|row| row.response.is_none())
        );
    }

    #[test]
    fn detail_loading_keeps_calendar_identity_when_uids_are_shared() {
        let (_tmp, mut state) = state(&[
            "BEGIN:VEVENT\nUID:event\nDTSTART;VALUE=DATE:20261025\nEND:VEVENT",
            "BEGIN:VTODO\nUID:task\nEND:VTODO",
        ]);
        for calendar in ["work", "home"] {
            for uid in ["event", "task"] {
                state
                    .store_mut()
                    .directory_mut(&Arc::new(calendar.into()))
                    .unwrap()
                    .file_by_id_mut(uid)
                    .unwrap()
                    .calendar_mut()
                    .components_mut()[0]
                    .set_summary(Some(calendar.into()));
            }
        }
        for calendar in ["work", "home"] {
            let mut item = eventix_state::actions::items::ItemId {
                calendar: calendar.into(),
                uid: "event".into(),
                recurrence: None,
            };
            assert_eq!(crate::Event::load(&state, &item).unwrap().summary, calendar);
            item.uid = "task".into();
            assert_eq!(
                crate::sidebar::SidebarRow::load(&state, &item)
                    .unwrap()
                    .summary,
                calendar
            );
        }
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
