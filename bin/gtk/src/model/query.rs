// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{DateTime, Duration};
use chrono_tz::Tz;
use eventix_ical::objects::{CalCompType, CalTodoStatus, EventLike};
use eventix_state::{State, actions::items::ItemId};

/// Identifies an active query whose snapshot can be refreshed independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SubscriptionId {
    /// Uses the content selection's generation so obsolete invalidations are ignored.
    Calendar(u64),
    /// The sidebar is independent of calendar navigation and has no generation.
    Sidebar,
}

pub(crate) enum Query {
    Events {
        start: DateTime<Tz>,
        end: DateTime<Tz>,
    },
    Sidebar {
        now: DateTime<Tz>,
    },
}

impl Query {
    /// Tests membership by identity against authoritative state, without copying objects.
    fn contains(&self, state: &State, item: &ItemId) -> bool {
        if state.misc().calendar_disabled(&item.calendar) {
            return false;
        }

        let Some(file) = state
            .store()
            .directories()
            .iter()
            .find(|dir| dir.id().as_str() == item.calendar)
            .and_then(|dir| dir.file_by_id(&item.uid))
        else {
            return false;
        };

        let Some(occurrence) =
            file.occurrence_by_id(&item.uid, item.recurrence.as_ref(), state.timezone())
        else {
            return false;
        };

        // A series edit can affect the visible period even when its first occurrence is earlier.
        if item.recurrence.is_none()
            && occurrence.is_recurrent()
            && occurrence.ctype() == CalCompType::Todo
        {
            return matches!(self, Self::Sidebar { .. });
        }

        if item.recurrence.is_none()
            && occurrence.is_recurrent()
            && occurrence.ctype() == CalCompType::Event
        {
            let (start, end) = match self {
                Self::Events { start, end } => (*start, *end),
                Self::Sidebar { now } => (*now, *now + Duration::days(7)),
            };
            return file
                .occurrences_between(start, end, |component| component.uid() == &item.uid)
                .any(|occurrence| !occurrence.is_excluded());
        }

        if occurrence.is_excluded() {
            return false;
        }

        match self {
            Self::Events { start, end } => {
                occurrence.ctype() == CalCompType::Event && occurrence.overlaps(*start, *end)
            }
            Self::Sidebar { now } => match occurrence.ctype() {
                CalCompType::Event => occurrence.overlaps(*now, *now + Duration::days(7)),
                // Conservative membership includes all overdue and unplanned tasks.
                CalCompType::Todo => {
                    occurrence.todo_status() != Some(CalTodoStatus::Completed)
                        && occurrence
                            .occurrence_end()
                            .is_none_or(|due| due < *now + Duration::days(21))
                }
            },
        }
    }
}

/// Replaces subscriptions as views navigate; hidden calendar periods need no eager refresh.
#[derive(Default)]
pub(crate) struct Subscriptions(Vec<(SubscriptionId, Query)>);

impl Subscriptions {
    pub fn ids(&self) -> Vec<SubscriptionId> {
        self.0.iter().map(|(id, _)| *id).collect()
    }

    pub fn contains(&self, id: SubscriptionId) -> bool {
        self.0.iter().any(|(active, _)| *active == id)
    }

    pub fn register(&mut self, id: SubscriptionId, query: Query) {
        self.0.retain(|(old, _)| {
            !matches!(
                (old, id),
                (SubscriptionId::Calendar(_), SubscriptionId::Calendar(_))
                    | (SubscriptionId::Sidebar, SubscriptionId::Sidebar)
            )
        });
        self.0.push((id, query));
    }

    pub fn matching(&self, state: &State, item: &ItemId) -> Vec<SubscriptionId> {
        self.0
            .iter()
            .filter(|(_, query)| query.contains(state, item))
            .map(|(id, _)| *id)
            .collect()
    }
}
