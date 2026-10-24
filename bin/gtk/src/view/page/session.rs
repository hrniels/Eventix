// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};

use chrono::NaiveDate;
use eventix_state::actions::{
    Action,
    items::{DeleteItem, ItemId, UpdatePersonalAlarms},
};
use gtk::{glib, prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::model::session::{CalendarSession, ChangeSet, ContentRequest, Event};
use crate::view::details::{EventDetails, delete};

/// Owns the model connection, command state, and periodic refresh lifecycle.
#[derive(Default)]
pub(super) struct PageSession {
    pub(super) current: RefCell<Option<CalendarSession>>,
    pending_command: Cell<bool>,
    pending_reminders: RefCell<Option<glib::WeakRef<EventDetails>>>,
    timer: RefCell<Option<glib::SourceId>>,
}

impl PageSession {
    pub(super) fn save_reminders(&self, page: &CalendarPage, command: UpdatePersonalAlarms) {
        if !self.can_edit(page) {
            return;
        }
        let Some(popover) = page.imp().details.retain_reminders() else {
            return;
        };
        let Some(selected) = popover.selected_item() else {
            return;
        };
        if let Some(session) = self.current.borrow().as_ref() {
            self.pending_command.set(true);
            self.pending_reminders.replace(Some(popover.downgrade()));
            page.imp()
                .status
                .set_text(page.locale().translate("Applying changes…"));
            session.save_reminders(selected, command);
        }
    }

    pub(super) fn reload(&self, page: &CalendarPage) {
        page.dismiss_popovers();

        let imp = page.imp();
        let content = &imp.content;
        content.loading.set(true);
        imp.status
            .set_text(page.locale().translate("Loading calendar…"));

        if let Some(session) = self.current.borrow().as_ref() {
            session.reload();

            // A failed initial load or navigation has no subscription for the selected content.
            if content.loaded_generation.get() != Some(content.generation.get()) {
                session.show_content(ContentRequest {
                    generation: content.generation.get(),
                    date: content.date.get(),
                    view: content.view.get(),
                });
                session.refresh_sidebar();
            }
        }
    }

    pub(super) fn can_edit(&self, page: &CalendarPage) -> bool {
        !page.imp().content.loading.get()
            && !self.pending_command.get()
            && self.current.borrow().is_some()
    }

    pub(super) fn execute_command(&self, page: &CalendarPage, command: Box<dyn Action>) -> bool {
        if !self.can_edit(page) {
            return false;
        }

        let imp = page.imp();
        if let Some(session) = self.current.borrow().as_ref() {
            self.pending_command.set(true);
            imp.status
                .set_text(page.locale().translate("Applying changes…"));
            session.execute(command);

            true
        } else {
            false
        }
    }

    pub(super) fn confirm_delete(
        &self,
        page: &CalendarPage,
        item: ItemId,
        recurrent: bool,
        summary: &str,
    ) {
        if !self.can_edit(page) {
            return;
        }

        let Some(window) = page.root().and_downcast::<gtk::Window>() else {
            return;
        };

        let locale = page.locale();
        let weak = page.downgrade();
        delete::confirm(&window, locale.as_ref(), recurrent, summary, move || {
            if let Some(page) = weak.upgrade()
                && page.execute_command(Box::new(DeleteItem { item }))
            {
                page.dismiss_popovers();
            }
        });
    }

    pub(super) fn start(&self, page: &CalendarPage, prefix: String, date: Option<NaiveDate>) {
        let (session, responses) = CalendarSession::start(prefix);
        self.current.replace(Some(session));
        page.install_styles();

        let weak = page.downgrade();
        let timer = glib::timeout_add_seconds_local(60, move || {
            let Some(page) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };

            page.imp().session.refresh(&page);

            glib::ControlFlow::Continue
        });
        self.timer.replace(Some(timer));

        page.show_content(date);
        if let Some(session) = self.current.borrow().as_ref() {
            session.refresh_sidebar();
        }

        let weak = page.downgrade();
        glib::MainContext::default().spawn_local(async move {
            while let Ok(response) = responses.recv().await {
                let Some(page) = weak.upgrade() else { break };

                if page.imp().session.current.borrow().is_none() {
                    break;
                }

                page.imp().session.handle_event(&page, response);
            }
        });
    }

    fn refresh(&self, page: &CalendarPage) {
        let imp = page.imp();
        imp.content.update_today(page);

        if !imp.content.loading.get()
            && !self.pending_command.get()
            && let Some(session) = self.current.borrow().as_ref()
        {
            tracing::debug!("updating sidebar");
            session.refresh_sidebar();
        }
    }

    fn handle_event(&self, page: &CalendarPage, event: Event) {
        let imp = page.imp();
        match event {
            Event::RemindersSaved(result) => {
                self.pending_command.set(false);
                let popover = self
                    .pending_reminders
                    .take()
                    .and_then(|weak| weak.upgrade())
                    .filter(|popover| imp.details.is_current(popover));
                match result {
                    Ok((change, item)) => {
                        if let Some(popover) = popover {
                            popover.reminders_saved(&item, page.locale().as_ref());
                        }
                        self.refresh_subscriptions(change);
                    }
                    Err(error) => page.show_error("Could not apply changes", &error),
                }
            }
            Event::EditLoaded(id, result) => imp.editor.edit_loaded(page, id, result),
            Event::EditorLoaded(id, result) => imp.editor.editor_loaded(page, id, result),
            Event::EditSaved(id, result) => {
                let result = result.map(|(change, item)| (change, Some(item)));
                if let Some(change) = imp.editor.saved(page, id, result) {
                    self.refresh_subscriptions(change);
                }
            }
            Event::EditorSaved(id, result) => {
                let result = result.map(|change| (change, None));
                if let Some(change) = imp.editor.saved(page, id, result) {
                    self.refresh_subscriptions(change);
                }
            }
            Event::ContentLoaded(response) => imp.content.receive_content(page, response),
            Event::SidebarLoaded(result) => match result {
                Ok(sidebar) => {
                    imp.sidebar.set_snapshot(sidebar);
                    page.update_styles();
                }
                Err(error) => page.show_error("Could not load sidebar", &error),
            },
            Event::SourceLoaded(source) => imp.status.set_tooltip_text(Some(&source)),
            Event::ReloadFailed(error) => {
                imp.content.loading.set(false);
                page.show_error("Could not load local calendars", &error);
            }
            Event::Changed(change) => {
                self.pending_command.set(false);
                self.refresh_subscriptions(change);
            }
            Event::CommandFailed(error) => {
                self.pending_command.set(false);
                page.show_error("Could not apply changes", &error);
            }
        }
    }

    fn refresh_subscriptions(&self, change: ChangeSet) {
        if let Some(session) = self.current.borrow().as_ref() {
            session.refresh_subscriptions(change.affected);
        }
    }

    pub(super) fn stop(&self, page: &CalendarPage) {
        page.imp().editor.stop(page);
        self.current.take();
        self.pending_reminders.take();
        page.dismiss_popovers();

        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
    }
}
