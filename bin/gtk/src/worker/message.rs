// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Worker messages, request identities, and operation activity descriptions.

use std::fmt;

use chrono::NaiveDate;
use eventix_ical::objects::CalCompType;
use eventix_state::SyncResult;
use eventix_state::actions::{
    Action,
    items::{CreateItem, EditScope, ItemId, UpdateItem, UpdatePersonalAlarms},
};
use eventix_state::auth::AuthOutcome;

use super::{
    management::{self, Request as ManagementRequest},
    subscriptions::SubscriptionId,
};
use crate::model::{
    View, activity::Activity, import::PreparedImport, list, list_occurrences as occurrences,
};

#[derive(Clone, Debug)]
pub(crate) struct ContentRequest {
    /// Identifies the page's content selection; incremented for each show-content request.
    ///
    /// Responses echo this value so the page can reject results superseded by navigation.
    /// It also identifies the installed calendar subscription, surviving refreshes and reloads.
    pub generation: u64,
    pub date: Option<NaiveDate>,
    pub view: View,
    pub filter: list::Filter,
}

pub(crate) struct ListRowsRequest {
    /// Identifies the view's index revision so late row data cannot update a newer list.
    pub revision: u64,
    pub items: Vec<ItemId>,
}

impl fmt::Debug for ListRowsRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("ListRowsRequest");
        debug.field("revision", &self.revision);
        if self.items.len() > 2 {
            debug.field("item-count", &self.items.len());
        } else {
            debug.field("items", &self.items);
        }
        debug.finish()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OccurrencesRequest {
    pub revision: u64,
    pub token: u64,
    pub item: ItemId,
    pub cursor: occurrences::Cursor,
    pub direction: occurrences::Direction,
    pub count: usize,
}

#[derive(Debug)]
pub(super) enum Message {
    PrepareImport(String),
    Import(PreparedImport, String),
    SavePreferences(eventix_state::Preferences),
    PushItemReady(ItemId, eventix_state::SyncColResult),
    PushFinished(Result<eventix_state::PushResult, String>),
    ManageCalendar(ManagementRequest),
    ToggleCalendar(String),
    PrepareEditor(u64, CalCompType),
    CreateItem(u64, Box<CreateItem>),
    LoadEdit(u64, ItemId, EditScope),
    SaveEdit(u64, Box<UpdateItem>),
    SaveReminders(ItemId, UpdatePersonalAlarms),
    LoadDetails(u64, ItemId, CalCompType),
    LoadFilepath(ItemId),
    ShowContent(ContentRequest),
    LoadManagement(u64),
    LoadListRows(ListRowsRequest),
    LoadOccurrences(OccurrencesRequest),
    RefreshSidebar,
    RefreshSubscriptions(Vec<SubscriptionId>),
    Reload,
    SyncAll,
    SyncCollection(String),
    LoadSyncCollections,
    CancelSync,
    Authenticate(u64, String),
    CancelAuthentication(u64),
    AuthenticationFinished(u64, Result<AuthOutcome, String>),
    SyncFinished(Result<SyncResult, String>),
    Execute(Box<dyn Action>),
    ManagementCompleted(ManagementRequest, Result<bool, String>),
}

impl Message {
    pub(super) fn activity(&self, request: u64) -> Option<Activity> {
        let (operation, items, calendar) = match self {
            Self::Import(_, calendar) => ("Import", vec![], Some(calendar.clone())),
            Self::ManageCalendar(request) => (
                request.description(),
                vec![],
                Some(
                    if matches!(
                        request.operation,
                        management::Operation::Collection(_)
                            | management::Operation::Calendar(
                                management::CalendarOperation::Create(_)
                            )
                    ) {
                        request.collection.clone()
                    } else {
                        request.calendar.clone()
                    },
                ),
            ),
            Self::ToggleCalendar(calendar) => (
                "Calendar visibility changed",
                vec![],
                Some(calendar.clone()),
            ),
            Self::CreateItem(_, item) => (item.description(), vec![], Some(item.calendar.clone())),
            Self::SaveEdit(_, item) => (item.description(), vec![item.item.clone()], None),
            Self::SaveReminders(item, _) => ("Reminders updated", vec![item.clone()], None),
            Self::Execute(action) => (action.description(), action.affected_items().to_vec(), None),
            Self::SyncAll => ("Synchronizing calendars", vec![], None),
            Self::SyncCollection(collection) => {
                ("Synchronizing calendars", vec![], Some(collection.clone()))
            }
            Self::Authenticate(_, collection) => {
                ("Authenticating", vec![], Some(collection.clone()))
            }
            _ => return None,
        };
        let mut activity = Activity::new(request, operation);
        activity.set_items(items);
        if matches!(self, Self::Authenticate(..) | Self::SyncCollection(_))
            || matches!(
                self,
                Self::ManageCalendar(command)
                    if matches!(
                        command.operation,
                        management::Operation::Collection(_)
                            | management::Operation::Calendar(management::CalendarOperation::Create(_))
                    )
            )
        {
            activity.set_collection(calendar);
        } else {
            activity.set_calendar(calendar);
        }
        Some(activity)
    }
}
