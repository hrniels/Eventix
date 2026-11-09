// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use chrono::{DateTime, NaiveDate, Utc};
use eventix_ical::objects::CalCompType;
use eventix_locale::{DateFlags, TimeFlags};
use eventix_state::actions::{
    Action,
    items::{DeleteItem, ItemId, UpdatePersonalAlarms},
};
use eventix_state::{SyncColResult, SyncResult};
use eventix_view::calendars::Backend;
use gtk::{glib, prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::model::session::{CalendarSession, ChangeSet, ContentRequest, Event, SavedItem};
use crate::view::auth::SyncPopup;
use crate::view::details::{EventDetails, delete};

/// Owns the model connection, command state, and periodic refresh lifecycle.
#[derive(Default)]
pub(super) struct PageSession {
    preferences: Cell<Option<eventix_state::Preferences>>,
    settings_popup: RefCell<Option<Rc<crate::view::settings::SettingsPopup>>>,
    pub(super) current: RefCell<Option<CalendarSession>>,
    pending_command: Cell<bool>,
    pending_quick_task: Cell<bool>,
    pending_reminders: RefCell<Option<glib::WeakRef<EventDetails>>>,
    details_request: Cell<u64>,
    pending_details_load: Cell<Option<u64>>,
    timer: RefCell<Option<glib::SourceId>>,
    sync_timer: RefCell<Option<glib::SourceId>>,
    pending_sync: Cell<bool>,
    sync_collection: RefCell<Option<String>>,
    last_sync: Cell<Option<DateTime<Utc>>>,
    auth_popup: RefCell<Option<Rc<SyncPopup>>>,
    auth_generation: Cell<u64>,
}

impl PageSession {
    pub(super) fn open_settings(&self, page: &CalendarPage) {
        if let Some(popup) = self.settings_popup.borrow().as_ref()
            && popup.is_open()
        {
            popup.present();
            return;
        }
        let Some(preferences) = self.preferences.get() else {
            return;
        };
        let Some(window) = page.root().and_downcast::<gtk::Window>() else {
            return;
        };
        page.dismiss_popovers();
        let weak = page.downgrade();
        let popup = crate::view::settings::SettingsPopup::new(
            &window,
            page.locale().as_ref(),
            preferences,
            move |preferences| {
                weak.upgrade().is_some_and(|page| {
                    page.imp()
                        .session
                        .current
                        .borrow()
                        .as_ref()
                        .is_some_and(|session| session.save_preferences(preferences))
                })
            },
        );
        self.settings_popup.replace(Some(popup));
    }

    fn update_preferences(&self, page: &CalendarPage, preferences: eventix_state::Preferences) {
        self.preferences.set(Some(preferences));
        page.imp().settings_button.set_sensitive(true);
        if let Some(timer) = self.sync_timer.take() {
            timer.remove();
        }
        let weak = page.downgrade();
        let timer =
            glib::timeout_add_seconds_local(preferences.sync_interval_minutes * 60, move || {
                let Some(page) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if !page.imp().session.pending_sync.get() {
                    page.imp().session.sync_collection.replace(None);
                    page.imp().session.synchronize(&page, false);
                }
                glib::ControlFlow::Continue
            });
        self.sync_timer.replace(Some(timer));
    }

    pub(super) fn update_sync_time(&self, page: &CalendarPage) {
        if let Some(time) = self.last_sync.get() {
            let locale = page.locale();
            let time = time.with_timezone(locale.timezone());
            page.imp()
                .last_sync
                .set_text(&locale.fmt_time(&time, TimeFlags::None));
            let tooltip = formatx::formatx!(
                locale.translate("Synchronize all calendars (last sync at: {})"),
                locale.fmt_datetime(&time, DateFlags::NoToday)
            )
            .unwrap();
            page.imp().reload.set_tooltip_text(Some(&tooltip));
        }
    }

    fn record_sync_time(&self, page: &CalendarPage) {
        self.last_sync.set(Some(Utc::now()));
        self.update_sync_time(page);
    }

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
            session.save_reminders(selected, command);
        }
    }

    pub(super) fn reload(&self, page: &CalendarPage) {
        if self.pending_sync.get() {
            return;
        }
        self.sync_collection.replace(None);
        self.synchronize(page, true);
    }

    pub(super) fn connect_sync_collections(&self, page: &CalendarPage) {
        let weak = page.downgrade();
        // GtkPopover::show runs after GTK has already presented the native popup.
        // Prepare its size and initial content before that first presentation instead.
        page.imp().sync_collections.set_create_popup_func(move |_| {
            if let Some(page) = weak.upgrade() {
                let imp = page.imp();
                if let Some(sync_control) = imp.sync_collections.parent() {
                    imp.sync_popover.set_width_request(sync_control.width());
                }
                if imp.sync_collection_list.first_child().is_none() {
                    let loading =
                        gtk::Button::with_label(page.locale().translate("Loading collections…"));
                    loading.set_sensitive(false);
                    loading.add_css_class("ev_details_button");
                    imp.sync_collection_list.append(&loading);
                }
                if let Some(session) = imp.session.current.borrow().as_ref() {
                    session.load_sync_collections();
                }
            }
        });
    }

    fn update_sync_collections(&self, page: &CalendarPage, collections: Vec<(String, Backend)>) {
        let imp = page.imp();
        while let Some(child) = imp.sync_collection_list.first_child() {
            imp.sync_collection_list.remove(&child);
        }
        for (collection, backend) in collections {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let icon = crate::view::calendars::collection_icon(backend);
            icon.set_pixel_size(20);
            row.append(&icon);
            let label = gtk::Label::new(Some(&collection));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            row.append(&label);
            let button = gtk::Button::builder().child(&row).build();
            button.update_property(&[gtk::accessible::Property::Label(&collection)]);
            button.add_css_class("ev_details_button");
            let weak = page.downgrade();
            button.connect_clicked(move |_| {
                if let Some(page) = weak.upgrade() {
                    let session = &page.imp().session;
                    if !session.pending_sync.get() {
                        session.sync_collection.replace(Some(collection.clone()));
                        page.imp().sync_collections.popdown();
                        session.synchronize(&page, true);
                    }
                }
            });
            imp.sync_collection_list.append(&button);
        }
    }

    fn synchronize(&self, page: &CalendarPage, dismiss_popovers: bool) {
        if self.pending_sync.get() {
            return;
        }
        if let Some(session) = self.current.borrow().as_ref() {
            let sent = if let Some(collection) = self.sync_collection.borrow().as_ref() {
                session.sync_collection(collection.clone())
            } else {
                session.sync_all()
            };
            if !sent {
                return;
            }
        } else {
            return;
        }
        if dismiss_popovers {
            page.dismiss_popovers();
        }
        let imp = page.imp();
        self.pending_sync.set(true);
        imp.reload.set_sensitive(false);
        imp.sync_collections.set_sensitive(false);
    }

    pub(super) fn cancel_sync(&self, page: &CalendarPage) {
        if page.imp().status.can_cancel()
            && let Some(session) = self.current.borrow().as_ref()
        {
            session.cancel_sync();
            page.imp().status.cancelling();
        }
    }

    fn sync_finished(&self, page: &CalendarPage, result: Result<SyncResult, String>) {
        let imp = page.imp();
        imp.management.refresh(page);
        self.pending_sync.set(false);
        imp.reload.set_sensitive(true);
        imp.sync_collections.set_sensitive(true);
        let cancelled = result.as_ref().is_ok_and(|result| result.cancelled);
        imp.status.finish_sync(result.is_ok(), cancelled);
        if result.is_ok() {
            self.record_sync_time(page);
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

        if cancelled {
            return;
        }
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
        page.imp().sync_collections.set_sensitive(false);
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
        page.imp().sync_collections.set_sensitive(true);
        if authenticated {
            self.synchronize(page, true);
        }
    }

    pub(super) fn can_edit(&self, page: &CalendarPage) -> bool {
        !page.management_open()
            && !page.imp().content.loading.get()
            && !self.pending_command.get()
            && self.current.borrow().is_some()
    }

    pub(super) fn execute_command(&self, page: &CalendarPage, command: Box<dyn Action>) -> bool {
        if !self.can_edit(page) {
            return false;
        }

        if let Some(session) = self.current.borrow().as_ref() {
            self.pending_command.set(true);
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
        page.imp().status.clear(page.locale().as_ref());
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
        // The worker publishes errors as activity before these results, including stale ones.
        // Result handlers only update their own controls and must not record the error again.
        let imp = page.imp();
        match event {
            Event::Alarm(alarm) => {
                if let Some(application) = page
                    .root()
                    .and_downcast::<gtk::Window>()
                    .and_then(|window| window.application())
                {
                    let notification = gtk::gio::Notification::new(&alarm.summary);
                    notification.set_body(Some(&alarm.body));
                    notification.set_priority(gtk::gio::NotificationPriority::Urgent);
                    application.send_notification(None, &notification);
                }
            }
            Event::PreferencesLoaded(preferences) => self.update_preferences(page, preferences),
            Event::PreferencesSaved(result) => {
                if let Ok(preferences) = &result {
                    self.update_preferences(page, *preferences);
                }
                if let Some(popup) = self.settings_popup.borrow().as_ref() {
                    popup.finished(&result);
                }
            }
            Event::Activity(activity) => imp.status.record(&activity, page.locale().as_ref()),
            Event::CalendarsLoaded(calendars) => {
                imp.status.set_calendars(&calendars);
                imp.calendar_toggles.update(page, calendars);
            }
            Event::CalendarToggleFailed(_) => {}
            Event::ManagementLoaded(generation, result) => {
                imp.management.receive(page, generation, result);
            }
            Event::ManagementFinished(request, result) => {
                imp.management.finished(page, &request, &result);
            }
            Event::FilepathLoaded(result) => {
                if let Ok(path) = result {
                    page.clipboard().set_text(&path);
                }
            }
            Event::DetailsLoaded(id, result) => {
                if self.pending_details_load.get() != Some(id) {
                    return;
                }
                self.pending_details_load.set(None);
                if let Ok(item) = result {
                    let locale = page.locale();
                    let details = match item {
                        SavedItem::Event(event) => EventDetails::new(&event, locale.as_ref()),
                        SavedItem::Task(task) => EventDetails::new_sidebar(&task, locale.as_ref()),
                    };
                    imp.details
                        .show_details(page.upcast_ref(), page.upcast_ref(), details);
                }
            }
            Event::RemindersSaved(result) => {
                self.pending_command.set(false);
                let popover = self
                    .pending_reminders
                    .take()
                    .and_then(|weak| weak.upgrade())
                    .filter(|popover| imp.details.is_current(popover));
                if let Ok((change, item)) = result {
                    if let Some(popover) = popover {
                        popover.reminders_saved(&item, page.locale().as_ref());
                    }
                    self.refresh_subscriptions(change);
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
                if let Some(list) = imp.list.get() {
                    let _ = list.set_rows(response);
                }
            }
            Event::OccurrencesLoaded(response) => {
                if let Some(list) = imp.list.get() {
                    let _ = list.set_occurrences(response);
                }
            }
            Event::SidebarLoaded(result) => {
                if let Ok(sidebar) = result {
                    if let Some(tray) = imp.tray.borrow().as_ref() {
                        tray.update(&sidebar);
                    }
                    imp.sidebar.set_snapshot(sidebar);
                    page.update_styles();
                }
            }
            Event::SyncCollectionsLoaded(collections) => {
                self.update_sync_collections(page, collections)
            }
            Event::StateLoaded => {
                self.record_sync_time(page);
                imp.management.refresh(page);
            }
            Event::ReloadFailed(_) => {
                imp.content.loading.set(false);
            }
            Event::SyncProgress { completed, total } => {
                imp.status.set_progress(completed, total);
            }
            Event::PendingPushes(count) => {
                imp.status.set_pending_pushes(count, page.locale().as_ref());
            }
            Event::PushAuthenticationRequired(collection) => self.authenticate(page, collection),
            Event::OperationStarted {
                description,
                determinate,
            } => {
                imp.status
                    .start_operation(page.locale().as_ref(), description, determinate);
            }
            Event::OperationFinished {
                completed,
                cancelled,
            } => {
                imp.status.finish_sync(completed, cancelled);
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
            Event::CommandFailed(_) => {
                self.pending_command.set(false);
                if self.pending_quick_task.replace(false) {
                    imp.sidebar.quick_task().finish(false);
                }
            }
        }
    }

    fn refresh_subscriptions(&self, change: ChangeSet) {
        if let Some(session) = self.current.borrow().as_ref() {
            session.refresh_subscriptions(change.affected);
        }
    }

    pub(super) fn stop(&self, page: &CalendarPage) {
        self.settings_popup.take();
        self.preferences.set(None);
        page.imp().settings_button.set_sensitive(false);
        page.imp().editor.stop(page);
        self.auth_popup.take();
        if let Some(session) = self.current.take() {
            session.cancel_sync();
        }
        self.pending_sync.set(false);
        page.imp().status.stop();
        page.imp().reload.set_sensitive(true);
        page.imp().sync_collections.set_sensitive(true);
        page.imp().sync_collections.popdown();
        if self.pending_quick_task.replace(false) {
            page.imp().sidebar.quick_task().finish(false);
        }
        self.pending_reminders.take();
        page.dismiss_popovers();

        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
        if let Some(timer) = self.sync_timer.take() {
            timer.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gtk::{glib, prelude::*, subclass::prelude::*};

    use super::{CalendarSession, Event};

    #[test]
    #[ignore = "requires a GTK display"]
    fn settings_toolbar_opens_only_one_popup_and_replaces_the_sync_timer() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::new(None::<&str>, Default::default());
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let (window, page) = crate::window::Window::test_new(&app);
        let imp = page.imp();
        assert_eq!(
            imp.settings_button.next_sibling().as_ref(),
            imp.calendars_button.parent().as_ref()
        );
        assert_ne!(imp.settings_button.parent(), imp.calendars_button.parent());
        let settings_icon = imp
            .settings_button
            .child()
            .unwrap()
            .downcast::<gtk::Image>()
            .unwrap();
        assert_eq!(
            settings_icon.resource().as_deref(),
            Some(resource_path!("icons/gear-symbolic.svg"))
        );
        let preferences = eventix_state::Preferences::default();
        imp.session.update_preferences(&page, preferences);
        let timer = imp.session.sync_timer.borrow().as_ref().unwrap().as_raw();
        window.present();
        imp.settings_button.emit_clicked();
        let popup = imp.session.settings_popup.borrow().clone().unwrap();
        imp.settings_button.emit_clicked();
        assert!(std::rc::Rc::ptr_eq(
            &popup,
            imp.session.settings_popup.borrow().as_ref().unwrap()
        ));
        imp.session.handle_event(
            &page,
            Event::PreferencesSaved(Ok(eventix_state::Preferences {
                sync_interval_minutes: 30,
                ..preferences
            })),
        );
        assert!(!popup.is_open());
        assert_ne!(
            timer,
            imp.session.sync_timer.borrow().as_ref().unwrap().as_raw()
        );
        imp.session.stop(&page);
        assert!(imp.session.sync_timer.borrow().is_none());
        window.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn sync_dropdown_lists_collections_and_selects_only_the_clicked_collection() {
        use eventix_view::calendars::Backend;

        let row_label = |button: &gtk::Button| {
            let row = button.child().and_downcast::<gtk::Box>().unwrap();
            let icon = row.first_child().and_downcast::<gtk::Image>().unwrap();
            assert!(icon.paintable().is_some());
            assert_eq!(icon.pixel_size(), 20);
            let label = icon.next_sibling().and_downcast::<gtk::Label>().unwrap();
            assert_eq!(label.xalign(), 0.0);
            assert!(label.hexpands());
            label.text()
        };
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("org.eventix.SyncDropdownTest")
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let (window, page) = crate::window::Window::test_new(&app);
        let imp = page.imp();
        window.present();
        while glib::MainContext::default().iteration(false) {}
        let reload = imp.reload.compute_bounds(&page).unwrap();
        let dropdown = imp.sync_collections.compute_bounds(&page).unwrap();
        assert!(dropdown.x() >= reload.x() + reload.width());
        assert!(dropdown.width() < reload.width());
        assert!(!imp.sync_popover.has_arrow());
        assert_eq!(imp.sync_popover.position(), gtk::PositionType::Bottom);
        assert_eq!(imp.sync_popover.halign(), gtk::Align::End);
        assert!(imp.sync_popover.has_css_class("ev_details_menu"));
        let initial_popup = std::rc::Rc::new(std::cell::Cell::new(None));
        let initial = initial_popup.clone();
        let weak = page.downgrade();
        imp.sync_popover.connect_realize(move |popover| {
            let page = weak.upgrade().unwrap();
            initial.set(Some((
                popover.width_request(),
                page.imp().sync_collection_list.first_child().is_some(),
            )));
        });
        let toggle = imp
            .sync_collections
            .first_child()
            .and_downcast::<gtk::ToggleButton>()
            .unwrap();
        toggle.emit_clicked();
        assert_eq!(
            initial_popup.get(),
            Some((imp.sync_collections.parent().unwrap().width(), true)),
            "the popup must have its width and content before GTK realizes its native surface"
        );
        assert_eq!(
            imp.sync_popover.width_request(),
            imp.sync_collections.parent().unwrap().width()
        );
        assert!(imp.sync_popover.width_request() >= (reload.width() + dropdown.width()) as i32);
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(100)));
        let loading = imp
            .sync_collection_list
            .first_child()
            .and_downcast::<gtk::Button>()
            .expect("the first opening must have content before collections arrive");
        assert_eq!(loading.label().as_deref(), Some("Loading collections…"));
        assert!(!loading.is_sensitive());
        assert!(loading.is_mapped());
        assert!(loading.height() > 0);
        imp.session.update_sync_collections(
            &page,
            vec![
                ("Personal".into(), Backend::FileSystem),
                ("Work".into(), Backend::CalDav),
            ],
        );
        glib::MainContext::default()
            .block_on(glib::timeout_future(std::time::Duration::from_millis(100)));
        assert!(
            imp.sync_popover.is_mapped(),
            "the first opening must stay visible"
        );
        assert!(imp.sync_collection_list.height() > 0);
        let personal = imp
            .sync_collection_list
            .first_child()
            .and_downcast::<gtk::Button>()
            .unwrap();
        let work = personal
            .next_sibling()
            .and_downcast::<gtk::Button>()
            .unwrap();
        assert_eq!(row_label(&personal), "Personal");
        assert_eq!(row_label(&work), "Work");
        for button in [&personal, &work] {
            assert!(button.has_css_class("ev_details_button"));
            assert!(!button.has_css_class("flat"));
        }
        assert!(work.next_sibling().is_none());
        work.emit_clicked();
        assert_eq!(
            imp.session.sync_collection.borrow().as_deref(),
            Some("Work")
        );
        imp.session.reload(&page);
        assert!(imp.session.sync_collection.borrow().is_none());
        imp.session
            .update_sync_collections(&page, vec![("Renamed".into(), Backend::O365)]);
        let renamed = imp
            .sync_collection_list
            .first_child()
            .and_downcast::<gtk::Button>()
            .unwrap();
        assert_eq!(row_label(&renamed), "Renamed");
        assert!(renamed.next_sibling().is_none());
        window.close();
    }

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
        let status = imp.status.imp();
        let locale = eventix_locale::new_with_timezone(
            &eventix_state::with_test_xdg(tmp.path(), tmp.path()),
            eventix_locale::LocaleType::English,
            chrono_tz::UTC,
        )
        .unwrap();
        imp.locale.replace(Some(locale));
        let previous_sync = chrono::DateTime::from_timestamp(0, 0).unwrap();
        imp.session.last_sync.set(Some(previous_sync));
        imp.session.update_sync_time(&page);
        let previous_label = imp.last_sync.text();
        assert!(!status.sync_progress.is_visible());
        assert!(!status.cancel_sync.is_visible());
        gtk::prelude::ActionGroupExt::activate_action(&window, "reload", None);
        assert!(imp.session.pending_sync.get());
        glib::MainContext::default().block_on(async {
            while let Ok(event) = responses.recv().await {
                let started = matches!(event, Event::OperationStarted { .. });
                imp.session.handle_event(&page, event);
                if started {
                    break;
                }
            }
        });
        assert!(status.sync_progress.is_visible());
        assert!(status.cancel_sync.is_visible());
        assert!(status.cancel_sync.is_sensitive());
        assert_eq!(status.sync_progress.fraction(), 0.0);
        assert!(!imp.reload.is_sensitive());
        gtk::prelude::ActionGroupExt::activate_action(&window, "reload", None);

        let context = glib::MainContext::default();
        while context.iteration(false) {}
        let progress_bounds = status.sync_progress.compute_bounds(&page).unwrap();
        let status_bounds = status.status.compute_bounds(&page).unwrap();
        assert!(status_bounds.x() + status_bounds.width() <= progress_bounds.x());
        let cancel_bounds = status.cancel_sync.compute_bounds(&page).unwrap();
        assert!(progress_bounds.x() + progress_bounds.width() <= cancel_bounds.x());
        assert_eq!(
            status.cancel_sync.icon_name().as_deref(),
            Some("window-close-symbolic")
        );

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
        assert_eq!(imp.status.text(), "Calendar synchronization completed");
        assert!(imp.session.last_sync.get().unwrap() > previous_sync);
        assert_ne!(imp.last_sync.text(), previous_label);
        let locale = page.locale();
        assert_eq!(
            imp.last_sync.text(),
            locale.fmt_time(
                &imp.session
                    .last_sync
                    .get()
                    .unwrap()
                    .with_timezone(locale.timezone()),
                eventix_locale::TimeFlags::None,
            )
        );
        while context.iteration(false) {}
        let reload_bounds = imp.reload.compute_bounds(&page).unwrap();
        let time_bounds = imp.last_sync.compute_bounds(&page).unwrap();
        assert!(time_bounds.x() >= reload_bounds.x());
        assert!(time_bounds.x() + time_bounds.width() <= reload_bounds.x() + reload_bounds.width());
        assert_eq!(
            imp.reload.tooltip_text().unwrap(),
            format!(
                "Synchronize all calendars (last sync at: {})",
                locale.fmt_datetime(
                    &imp.session
                        .last_sync
                        .get()
                        .unwrap()
                        .with_timezone(locale.timezone()),
                    eventix_locale::DateFlags::NoToday,
                )
            )
        );
        assert!(!imp.session.pending_sync.get());
        assert!(imp.reload.is_sensitive());
        assert_eq!(status.sync_progress.fraction(), 1.0);
        assert!(status.sync_progress.is_visible());
        assert!(!status.cancel_sync.is_visible());
        context.block_on(glib::timeout_future(std::time::Duration::from_millis(1100)));
        assert!(!status.sync_progress.is_visible());
        assert!(status.progress_timer.borrow().is_none());

        let last_sync = imp.session.last_sync.get();
        imp.session.sync_finished(&page, Err("test failure".into()));
        assert_eq!(imp.session.last_sync.get(), last_sync);
        assert!(!status.sync_progress.is_visible());
        let latest = imp.status.text();
        imp.session.pending_sync.set(true);
        imp.reload.set_sensitive(false);
        status.sync_progress.set_fraction(0.5);
        status.sync_progress.set_visible(true);
        status.cancel_sync.set_visible(true);
        status.cancel_sync.emit_clicked();
        assert!(!status.cancel_sync.is_sensitive());
        assert!(!imp.reload.is_sensitive());
        assert!(imp.session.pending_sync.get());
        assert_eq!(
            imp.status.text(),
            latest,
            "only activity events replace the latest entry"
        );
        status.cancel_sync.emit_clicked();
        imp.session.sync_finished(
            &page,
            Ok(eventix_state::SyncResult {
                cancelled: true,
                collections: [(
                    "office".into(),
                    eventix_state::SyncColResult::AuthFailed("https://example.invalid/auth".into()),
                )]
                .into(),
                ..Default::default()
            }),
        );
        assert!(!imp.session.pending_sync.get());
        assert!(imp.reload.is_sensitive());
        assert!(!status.sync_progress.is_visible());
        assert_eq!(status.sync_progress.fraction(), 0.5);
        assert!(!status.cancel_sync.is_visible());
        assert!(imp.session.auth_popup.borrow().is_none());
        assert!(status.progress_timer.borrow().is_none());
        assert_eq!(imp.status.text(), latest);
        for description in [
            "Reloading calendar…",
            "Reloading collection…",
            "Discovering collection…",
            "Synchronizing collection…",
        ] {
            imp.session.handle_event(
                &page,
                Event::OperationStarted {
                    description,
                    determinate: false,
                },
            );
            assert!(!imp.session.pending_sync.get());
            assert!(status.sync_progress.is_visible());
            assert!(status.cancel_sync.is_visible());
            assert!(status.cancel_sync.is_sensitive());
            assert!(status.progress_timer.borrow().is_some());
            assert_eq!(
                status.sync_progress.tooltip_text().as_deref(),
                Some(description)
            );
            status.cancel_sync.emit_clicked();
            assert!(!status.cancel_sync.is_sensitive());
            imp.session.handle_event(
                &page,
                Event::OperationFinished {
                    completed: true,
                    cancelled: true,
                },
            );
            assert!(!status.sync_progress.is_visible());
            assert!(!status.cancel_sync.is_visible());
            assert!(status.progress_timer.borrow().is_none());
        }
        window.close();
        assert!(status.progress_timer.borrow().is_none());
        assert!(!status.sync_progress.is_visible());
    }
}
