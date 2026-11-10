// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UI-facing session handle for submitting requests and receiving worker events.

use std::{sync::Arc, thread};

use eventix_ical::objects::CalCompType;
use eventix_state::actions::{
    Action,
    items::{CreateItem, EditScope, ItemId, UpdateItem, UpdatePersonalAlarms},
};

use super::{
    event::Event,
    management::Request as ManagementRequest,
    message::{ContentRequest, ListRowsRequest, Message, OccurrencesRequest},
    runtime::Worker,
    subscriptions::SubscriptionId,
};
use crate::model::import::PreparedImport;

/// Owns the request channel, closing the worker when the page stops or is dropped.
pub(crate) struct Session {
    requests: async_channel::Sender<Message>,
}

impl Session {
    pub fn prepare_import(&self, input: String) {
        let _ = self.requests.try_send(Message::PrepareImport(input));
    }

    pub fn import(&self, preview: PreparedImport, calendar: String) {
        let _ = self.requests.try_send(Message::Import(preview, calendar));
    }

    pub fn start(prefix: String) -> (Self, async_channel::Receiver<Event>) {
        let xdg = Arc::new(xdg::BaseDirectories::with_prefix(prefix));
        Self::start_worker(Worker::new(xdg))
    }

    #[cfg(test)]
    pub(crate) fn test_start(
        state: eventix_state::State,
    ) -> (Self, async_channel::Receiver<Event>) {
        Self::start_worker(Worker::with_state(state))
    }

    pub(super) fn start_worker(worker: Worker) -> (Self, async_channel::Receiver<Event>) {
        let (requests, rx) = async_channel::unbounded::<Message>();
        let (results, responses) = async_channel::unbounded();
        let worker_requests = requests.clone();

        thread::spawn(move || worker.run(rx, worker_requests, results));

        (Self { requests }, responses)
    }

    pub fn toggle_calendar(&self, id: String) {
        let _ = self.requests.try_send(Message::ToggleCalendar(id));
    }

    pub fn save_preferences(&self, preferences: eventix_state::Preferences) -> bool {
        self.requests
            .try_send(Message::SavePreferences(preferences))
            .is_ok()
    }

    pub fn manage_calendar(&self, request: ManagementRequest) {
        let _ = self.requests.try_send(Message::ManageCalendar(request));
    }

    pub fn load_filepath(&self, item: ItemId) {
        let _ = self.requests.try_send(Message::LoadFilepath(item));
    }

    pub fn load_details(&self, id: u64, item: ItemId, kind: CalCompType) {
        let _ = self.requests.try_send(Message::LoadDetails(id, item, kind));
    }

    pub fn save_reminders(&self, selected: ItemId, command: UpdatePersonalAlarms) {
        let _ = self
            .requests
            .try_send(Message::SaveReminders(selected, command));
    }

    pub fn load_edit(&self, id: u64, item: ItemId, scope: EditScope) {
        let _ = self.requests.try_send(Message::LoadEdit(id, item, scope));
    }

    pub fn save_edit(&self, id: u64, edit: Box<UpdateItem>) {
        let _ = self.requests.try_send(Message::SaveEdit(id, edit));
    }

    pub fn prepare_editor(&self, id: u64, kind: CalCompType) {
        let _ = self.requests.try_send(Message::PrepareEditor(id, kind));
    }

    pub fn create_item(&self, id: u64, item: Box<CreateItem>) {
        let _ = self.requests.try_send(Message::CreateItem(id, item));
    }

    pub fn show_content(&self, request: ContentRequest) {
        let _ = self.requests.try_send(Message::ShowContent(request));
    }

    pub fn load_management(&self, generation: u64) {
        let _ = self.requests.try_send(Message::LoadManagement(generation));
    }

    pub fn load_list_rows(&self, request: ListRowsRequest) {
        let _ = self.requests.try_send(Message::LoadListRows(request));
    }

    pub fn load_occurrences(&self, request: OccurrencesRequest) {
        let _ = self.requests.try_send(Message::LoadOccurrences(request));
    }

    pub fn refresh_sidebar(&self) {
        let _ = self.requests.try_send(Message::RefreshSidebar);
    }

    pub fn sync_all(&self) -> bool {
        self.requests.try_send(Message::SyncAll).is_ok()
    }

    pub fn sync_collection(&self, collection: String) -> bool {
        self.requests
            .try_send(Message::SyncCollection(collection))
            .is_ok()
    }

    pub fn load_sync_collections(&self) {
        let _ = self.requests.try_send(Message::LoadSyncCollections);
    }

    pub fn cancel_sync(&self) {
        let _ = self.requests.try_send(Message::CancelSync);
    }

    pub fn authenticate(&self, id: u64, collection: String) {
        let _ = self
            .requests
            .try_send(Message::Authenticate(id, collection));
    }

    pub fn cancel_authentication(&self, id: u64) {
        let _ = self.requests.try_send(Message::CancelAuthentication(id));
    }

    pub fn execute(&self, command: Box<dyn Action>) {
        let _ = self.requests.try_send(Message::Execute(command));
    }

    pub fn refresh_subscriptions(&self, subscriptions: Vec<SubscriptionId>) {
        let _ = self
            .requests
            .try_send(Message::RefreshSubscriptions(subscriptions));
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.requests.close();
    }
}
