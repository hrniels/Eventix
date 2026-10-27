// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use chrono::NaiveDate;
use eventix_ical::objects::CalCompType;
use eventix_state::actions::{
    Action,
    items::{DeleteItem, ItemId, UpdatePersonalAlarms},
};
use eventix_state::{SyncColResult, SyncResult};
use gtk::{glib, prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::model::session::{CalendarSession, ChangeSet, ContentRequest, Event, SavedItem};
use crate::view::auth::SyncPopup;
use crate::view::details::{EventDetails, delete};

/// Owns the model connection, command state, and periodic refresh lifecycle.
#[derive(Default)]
pub(super) struct PageSession {
    pub(super) current: RefCell<Option<CalendarSession>>,
    pending_command: Cell<bool>,
    pending_quick_task: Cell<bool>,
    pending_reminders: RefCell<Option<glib::WeakRef<EventDetails>>>,
    details_request: Cell<u64>,
    pending_details_load: Cell<Option<u64>>,
    timer: RefCell<Option<glib::SourceId>>,
    pending_sync: Cell<bool>,
    progress_timer: RefCell<Option<glib::SourceId>>,
    auth_popup: RefCell<Option<Rc<SyncPopup>>>,
    auth_generation: Cell<u64>,
}

impl PageSession {
    pub(super) fn cancel_details_load(&self) {
        self.pending_details_load.set(None);
    }

    pub(super) fn load_details(&self, item: ItemId, kind: CalCompType) {
        if let Some(session) = self.current.borrow().as_ref() {
            let id = self.details_request.get().wrapping_add(1);
            self.details_request.set(id);
            self.pending_details_load.set(Some(id));
            session.load_details(id, item, kind);
        }
    }

    pub(super) fn connect_quick_task(&self, page: &CalendarPage) {
        let quick = page.imp().sidebar.quick_task();
        let weak = page.downgrade();
        quick.add.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.imp().session.add_quick_task(&page);
            }
        });
        let weak = page.downgrade();
        quick.summary.connect_activate(move |_| {
            if let Some(page) = weak.upgrade() {
                page.imp().session.add_quick_task(&page);
            }
        });
    }

    fn add_quick_task(&self, page: &CalendarPage) {
        if !self.can_edit(page) || self.pending_quick_task.get() {
            return;
        }
        let quick = page.imp().sidebar.quick_task();
        match quick.command(page.locale().as_ref()) {
            Ok(command) => {
                if self.execute_command(page, command) {
                    self.pending_quick_task.set(true);
                    quick.set_saving();
                }
            }
            Err(error) => page.show_error("Could not apply changes", &error.to_string()),
        }
    }

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
        if self.pending_sync.get() {
            return;
        }
        if let Some(session) = self.current.borrow().as_ref() {
            if !session.sync_all() {
                return;
            }
        } else {
            return;
        }
        page.dismiss_popovers();
        let imp = page.imp();
        self.pending_sync.set(true);
        if let Some(timer) = self.progress_timer.take() {
            timer.remove();
        }
        imp.reload.set_sensitive(false);
        imp.sync_progress.set_fraction(0.0);
        imp.sync_progress.set_visible(true);
        imp.sync_progress
            .set_tooltip_text(Some(page.locale().translate("Synchronizing calendars…")));
    }

    fn sync_finished(&self, page: &CalendarPage, result: Result<SyncResult, String>) {
        let imp = page.imp();
        if let Some(timer) = self.progress_timer.take() {
            timer.remove();
        }
        self.pending_sync.set(false);
        imp.reload.set_sensitive(true);
        let errors = match &result {
            Ok(result) => {
                imp.sync_progress.set_fraction(1.0);
                sync_errors(result, page.locale().as_ref())
            }
            Err(error) => {
                imp.sync_progress.set_visible(false);
                vec![error.clone()]
            }
        };
        if errors.is_empty() {
            imp.status
                .set_text(page.locale().translate("Synchronization completed"));
        } else {
            page.show_error("Could not synchronize calendars", &errors.join("\n"));
        }

        // A failed initial load or navigation has no subscription for the selected content.
        let content = &imp.content;
        if content.loaded_generation.get() != Some(content.generation.get())
            && let Some(session) = self.current.borrow().as_ref()
        {
            session.show_content(ContentRequest {
                generation: content.generation.get(),
                date: content.date.get(),
                view: content.view.get(),
                filter: imp.list.get().map(|list| list.filter()).unwrap_or_default(),
            });
            session.refresh_sidebar();
        }

        let weak = page.downgrade();
        self.progress_timer
            .replace(Some(glib::timeout_add_local_once(
                std::time::Duration::from_secs(1),
                move || {
                    if let Some(page) = weak.upgrade() {
                        page.imp().session.progress_timer.take();
                        page.imp().sync_progress.set_visible(false);
                    }
                },
            )));

        if let Ok(result) = &result {
            let mut collections: Vec<_> = result
                .collections
                .iter()
                .filter_map(|(collection, outcome)| {
                    matches!(outcome, SyncColResult::AuthFailed(_)).then_some(collection.clone())
                })
                .collect();
            collections.sort();
            if let Some(collection) = collections.into_iter().next() {
                self.authenticate(page, collection);
            }
        }
    }

    fn authenticate(&self, page: &CalendarPage, collection: String) {
        let Some(window) = page.root().and_downcast::<gtk::Window>() else {
            return;
        };
        if self.current.borrow().is_none() {
            return;
        }
        let id = self.auth_generation.get().wrapping_add(1);
        self.auth_generation.set(id);
        self.pending_sync.set(true);
        page.imp().reload.set_sensitive(false);
        page.imp()
            .status
            .set_text(page.locale().translate("Waiting for authentication..."));
        let weak = page.downgrade();
        let popup = SyncPopup::new(
            &window,
            page.locale(),
            collection.clone(),
            move |authenticated| {
                if let Some(page) = weak.upgrade() {
                    page.imp()
                        .session
                        .authentication_finished(&page, authenticated);
                }
            },
        );
        self.auth_popup.replace(Some(Rc::new(popup)));
        if let Some(session) = self.current.borrow().as_ref() {
            session.authenticate(id, collection);
        }
    }

    fn authentication_finished(&self, page: &CalendarPage, authenticated: bool) {
        self.auth_popup.take();
        if let Some(session) = self.current.borrow().as_ref() {
            session.cancel_authentication(self.auth_generation.get());
        }
        self.pending_sync.set(false);
        page.imp().reload.set_sensitive(true);
        if authenticated {
            self.reload(page);
        } else {
            page.imp()
                .status
                .set_text(page.locale().translate("Synchronization cancelled"));
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
            Event::CalendarsLoaded(calendars) => imp.calendar_toggles.update(page, calendars),
            Event::CalendarToggleFailed(error) => {
                page.show_error("Could not apply changes", &error);
            }
            Event::FilepathLoaded(result) => match result {
                Ok(path) => page.clipboard().set_text(&path),
                Err(error) => page.show_error("Could not copy filepath", &error),
            },
            Event::DetailsLoaded(id, result) => {
                if self.pending_details_load.get() != Some(id) {
                    return;
                }
                self.pending_details_load.set(None);
                match result {
                    Ok(item) => {
                        let locale = page.locale();
                        let details = match item {
                            SavedItem::Event(event) => EventDetails::new(&event, locale.as_ref()),
                            SavedItem::Task(task) => {
                                EventDetails::new_sidebar(&task, locale.as_ref())
                            }
                        };
                        imp.details
                            .show_details(page.upcast_ref(), page.upcast_ref(), details);
                    }
                    Err(error) => page.show_error("Could not apply changes", &error),
                }
            }
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
            Event::ListRowsLoaded(response) => {
                if let Some(list) = imp.list.get()
                    && let Err(error) = list.set_rows(response)
                {
                    page.show_error("Could not load local calendars", &error);
                }
            }
            Event::OccurrencesLoaded(response) => {
                if let Some(list) = imp.list.get()
                    && let Err(error) = list.set_occurrences(response)
                {
                    page.show_error("Could not load local calendars", &error);
                }
            }
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
            Event::SyncProgress { completed, total } => {
                imp.sync_progress.set_fraction(if total == 0 {
                    1.0
                } else {
                    completed as f64 / total as f64
                });
            }
            Event::SyncFinished(result) => self.sync_finished(page, result),
            Event::AuthPrompt(id, prompt) => {
                if id == self.auth_generation.get()
                    && let Some(popup) = self.auth_popup.borrow().as_ref()
                {
                    popup.set_prompt(prompt);
                }
            }
            Event::AuthFinished(id, result) => {
                let popup = self.auth_popup.borrow().clone();
                if id == self.auth_generation.get()
                    && let Some(popup) = popup
                {
                    popup.finished(result);
                }
            }
            Event::Changed(change) => {
                self.pending_command.set(false);
                if self.pending_quick_task.replace(false) {
                    imp.sidebar.quick_task().finish(true);
                }
                self.refresh_subscriptions(change);
            }
            Event::CommandFailed(error) => {
                self.pending_command.set(false);
                if self.pending_quick_task.replace(false) {
                    imp.sidebar.quick_task().finish(false);
                }
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
        self.auth_popup.take();
        self.current.take();
        self.pending_sync.set(false);
        page.imp().sync_progress.set_visible(false);
        page.imp().reload.set_sensitive(true);
        if let Some(timer) = self.progress_timer.take() {
            timer.remove();
        }
        if self.pending_quick_task.replace(false) {
            page.imp().sidebar.quick_task().finish(false);
        }
        self.pending_reminders.take();
        page.dismiss_popovers();

        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
    }
}

fn sync_errors(result: &SyncResult, locale: &dyn eventix_locale::Locale) -> Vec<String> {
    let mut errors = Vec::new();
    for (collection, outcome) in &result.collections {
        match outcome {
            SyncColResult::Success(_) => {}
            SyncColResult::Error(error) => errors.push(format!("{collection}: {error}")),
            SyncColResult::AuthFailed(_) => {}
        }
    }
    for (calendar, failed) in &result.calendars {
        if *failed {
            errors.push(
                formatx::formatx!(
                    locale.translate("Could not synchronize calendar '{}'"),
                    calendar
                )
                .unwrap(),
            );
        }
    }
    errors.sort();
    errors
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gtk::{glib, prelude::*, subclass::prelude::*};

    use super::{CalendarSession, Event};

    #[test]
    #[ignore = "requires a GTK display"]
    fn auth_failure_pauses_sync_until_authentication_and_cancel_suppresses_retry() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.hrniels.Eventix.AuthTest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let (window, page) = crate::window::Window::test_new(&app);
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let state = eventix_state::State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        let (session, responses) = CalendarSession::test_start(state);
        let imp = page.imp();
        imp.session.current.replace(Some(session));
        window.present();
        let failure = || {
            Ok(eventix_state::SyncResult {
                collections: [(
                    "office".to_string(),
                    eventix_state::SyncColResult::AuthFailed(
                        "https://example.invalid/auth".to_string(),
                    ),
                )]
                .into(),
                ..Default::default()
            })
        };
        imp.session.sync_finished(&page, failure());
        assert!(imp.session.pending_sync.get());
        assert!(imp.session.auth_popup.borrow().is_some());
        assert!(!imp.reload.is_sensitive());
        assert!(!imp.status.text().contains("example.invalid"));
        imp.session.authentication_finished(&page, false);
        assert!(imp.session.auth_popup.borrow().is_none());
        assert!(!imp.session.pending_sync.get());
        assert!(imp.reload.is_sensitive());

        imp.session.sync_finished(&page, failure());
        let id = imp.session.auth_generation.get();
        imp.session.handle_event(
            &page,
            Event::AuthFinished(id - 1, Ok(eventix_state::auth::AuthOutcome::Complete)),
        );
        assert!(
            imp.session.auth_popup.borrow().is_some(),
            "stale authentication is ignored"
        );
        imp.session.handle_event(
            &page,
            Event::AuthFinished(id, Ok(eventix_state::auth::AuthOutcome::Complete)),
        );
        assert!(imp.session.auth_popup.borrow().is_none());
        assert!(
            imp.session.pending_sync.get(),
            "successful authentication retries sync"
        );
        let context = glib::MainContext::default();
        context.block_on(async {
            while let Ok(event) = responses.recv().await {
                let finished = matches!(event, Event::SyncFinished(_));
                imp.session.handle_event(&page, event);
                if finished {
                    break;
                }
            }
        });
        assert!(!imp.session.pending_sync.get());
        assert!(imp.reload.is_sensitive());

        imp.session.sync_finished(&page, failure());
        window.close();
        assert!(imp.session.auth_popup.borrow().is_none());
        assert!(!imp.session.pending_sync.get());
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn refresh_action_syncs_in_background_and_displays_right_status_progress() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.hrniels.Eventix.SyncTest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let (window, page) = crate::window::Window::test_new(&app);
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let state = eventix_state::State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        let (session, responses) = CalendarSession::test_start(state);
        let imp = page.imp();
        imp.session.current.replace(Some(session));
        window.present();
        assert!(!imp.sync_progress.is_visible());
        gtk::prelude::ActionGroupExt::activate_action(&window, "reload", None);
        assert!(imp.session.pending_sync.get());
        assert!(imp.sync_progress.is_visible());
        assert_eq!(imp.sync_progress.fraction(), 0.0);
        assert!(!imp.reload.is_sensitive());
        gtk::prelude::ActionGroupExt::activate_action(&window, "reload", None);

        let context = glib::MainContext::default();
        while context.iteration(false) {}
        let progress_bounds = imp.sync_progress.compute_bounds(&page).unwrap();
        let status_bounds = imp.status.compute_bounds(&page).unwrap();
        assert!(status_bounds.x() + status_bounds.width() <= progress_bounds.x());

        let mut completed = false;
        context.block_on(async {
            while let Ok(event) = responses.recv().await {
                let finished = matches!(event, Event::SyncFinished(_));
                imp.session.handle_event(&page, event);
                if finished {
                    completed = true;
                    break;
                }
            }
        });
        assert!(completed);
        assert!(!imp.session.pending_sync.get());
        assert!(imp.reload.is_sensitive());
        assert_eq!(imp.sync_progress.fraction(), 1.0);
        assert!(imp.sync_progress.is_visible());
        context.block_on(glib::timeout_future(std::time::Duration::from_millis(1100)));
        assert!(!imp.sync_progress.is_visible());
        assert!(imp.session.progress_timer.borrow().is_none());

        imp.session.sync_finished(&page, Err("test failure".into()));
        assert!(!imp.sync_progress.is_visible());
        assert!(imp.status.text().contains("test failure"));
        window.close();
        assert!(imp.session.progress_timer.borrow().is_none());
        assert!(!imp.sync_progress.is_visible());
    }
}
