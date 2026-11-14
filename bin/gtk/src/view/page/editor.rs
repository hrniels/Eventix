// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use eventix_ical::objects::CalCompType;
use eventix_state::actions::items::{EditScope, ItemId, UpdateItem};
use gtk::{prelude::*, subclass::prelude::*};

use super::CalendarPage;
use super::create::EventSlot;
use crate::model::editor::EditorOptions;
use crate::view::editor::ItemEditor;
use crate::worker::event::ChangeSet;

/// Owns editor requests and the active editor's lifecycle.
#[derive(Default)]
pub(super) struct PageEditor {
    pub(super) current: RefCell<Option<Rc<ItemEditor>>>,
    window: RefCell<Option<gtk::Window>>,
    request: Cell<u64>,
    loading: Cell<bool>,
    slot: Cell<Option<EventSlot>>,
}

impl PageEditor {
    pub(super) fn edit_item(&self, page: &CalendarPage, item: ItemId, scope: EditScope) {
        if self.editor_open() {
            self.present_current();
            return;
        }

        if let Some(session) = page.imp().session.current.borrow().as_ref() {
            let id = self.begin_request();
            session.load_edit(id, item, scope);
        }
    }

    fn begin_request(&self) -> u64 {
        let id = self.request.get().wrapping_add(1);
        self.request.set(id);
        self.loading.set(true);

        id
    }

    pub(super) fn present_existing_editor(
        &self,
        page: &CalendarPage,
        id: u64,
        options: EditorOptions,
        edit: UpdateItem,
    ) {
        self.present_editor(page, ItemEditor::edit(id, options, edit));
    }

    pub(super) fn present_editor(&self, page: &CalendarPage, editor: Rc<ItemEditor>) {
        if self.editor_open() {
            self.present_current();
            return;
        }
        let Some(parent) = page.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let window = editor.create_window(&parent);
        let weak = page.downgrade();
        editor.cancel.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.close_editor();
            }
        });

        let weak = page.downgrade();
        editor.save.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.save_editor();
            }
        });

        let weak = page.downgrade();
        window.connect_close_request(move |_| {
            if let Some(page) = weak.upgrade() {
                page.close_editor();
            }
            gtk::glib::Propagation::Stop
        });

        page.dismiss_popovers();
        self.current.replace(Some(editor.clone()));
        self.window.replace(Some(window.clone()));
        // A load may complete after the owner has been hidden to the tray.
        if parent.is_visible() {
            window.present();
            editor.focus();
        }
    }

    fn present_current(&self) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.present();
        }
    }

    pub(super) fn set_owner_visible(&self, visible: bool) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.set_visible(visible);
        }
    }

    pub(super) fn saving(&self) -> bool {
        self.current
            .borrow()
            .as_ref()
            .is_some_and(|editor| editor.saving.get())
    }

    pub(super) fn editor_open(&self) -> bool {
        self.loading.get() || self.current.borrow().is_some()
    }

    pub(super) fn new_item(&self, page: &CalendarPage, kind: CalCompType) {
        self.new_item_at(page, kind, None);
    }

    pub(super) fn new_event_at(&self, page: &CalendarPage, slot: EventSlot) {
        self.new_item_at(page, CalCompType::Event, Some(slot));
    }

    fn new_item_at(&self, page: &CalendarPage, kind: CalCompType, slot: Option<EventSlot>) {
        if self.editor_open() {
            self.present_current();
            return;
        }

        if let Some(session) = page.imp().session.current.borrow().as_ref() {
            page.dismiss_popovers();
            let id = self.begin_request();
            self.slot.set(slot);
            session.prepare_editor(id, kind);
        }
    }

    pub(super) fn finish_editor(&self) {
        let editor = self.current.borrow().clone();
        if editor.as_ref().is_some_and(|editor| editor.saving.get()) {
            return;
        }

        if let Some(window) = self.window.take() {
            window.destroy();
        }
        self.current.take();

        self.loading.set(false);
        self.slot.set(None);
        self.request.set(self.request.get().wrapping_add(1));
    }

    pub(super) fn save_editor(&self, page: &CalendarPage) {
        let Some(editor) = self.current.borrow().clone() else {
            return;
        };

        if editor.saving.get() {
            return;
        }

        match editor.command() {
            Ok(command) => {
                editor.clear_errors();

                if let Some(session) = page.imp().session.current.borrow().as_ref() {
                    editor.set_saving(true);

                    if let Some(mut edit) = editor.existing.borrow().clone() {
                        edit.fields = *command;
                        session.save_edit(editor.id, Box::new(edit));
                    } else {
                        session.create_item(editor.id, command);
                    }
                }
            }
            Err(issue) => editor.show_validation_error(&issue),
        }
    }

    pub(super) fn edit_loaded(
        &self,
        page: &CalendarPage,
        id: u64,
        result: Result<(EditorOptions, Box<UpdateItem>), String>,
    ) {
        if id != self.request.get() {
            return;
        }

        self.loading.set(false);
        if let Ok((options, edit)) = result {
            self.present_existing_editor(page, id, options, *edit);
        }
    }

    pub(super) fn editor_loaded(
        &self,
        page: &CalendarPage,
        id: u64,
        result: Result<EditorOptions, String>,
    ) {
        if id != self.request.get() {
            return;
        }

        self.loading.set(false);
        if let Ok(mut options) = result {
            let slot = self.slot.take();
            if let Some(slot) = slot {
                options.start = slot.start;
            }

            let editor = if let Some(slot) = slot {
                ItemEditor::new_in_range(id, options, slot.all_day, slot.end)
            } else {
                ItemEditor::new(id, options)
            };
            self.present_editor(page, editor);
        }
    }

    pub(super) fn saved(&self, id: u64, result: Result<ChangeSet, String>) -> Option<ChangeSet> {
        let editor = self
            .current
            .borrow()
            .clone()
            .filter(|editor| editor.id == id);

        match result {
            Ok(change) => {
                if let Some(editor) = editor {
                    editor.set_saving(false);
                    self.finish_editor();
                }

                Some(change)
            }
            Err(error) => {
                if let Some(editor) = editor {
                    editor.set_saving(false);
                    editor.show_error(&error);
                }

                None
            }
        }
    }

    pub(super) fn stop(&self) {
        if let Some(editor) = self.current.borrow().as_ref() {
            editor.set_saving(false);
        }

        self.finish_editor();
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use eventix_ical::objects::CalCompType;
    use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

    use super::CalendarPage;
    use crate::model::{
        View,
        editor::{EditorCalendar, EditorOptions},
    };
    use crate::view::{details::EventDetails, editor::ItemEditor};
    use crate::worker::event::ChangeSet;

    fn options() -> EditorOptions {
        EditorOptions {
            kind: CalCompType::Event,
            calendars: vec![EditorCalendar {
                id: "test".into(),
                name: "Test".into(),
                background: "#27c281".into(),
                personal: false,
            }],
            last_calendar: None,
            locations: vec![],
            contacts: vec![],
            start: "2026-10-15T10:00:00".parse().unwrap(),
            locale: eventix_locale::default(),
        }
    }

    fn drain() {
        for _ in 0..30 {
            while glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn press(window: &impl IsA<gtk::Widget>, key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
        let keys = window
            .observe_controllers()
            .snapshot()
            .into_iter()
            .filter_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .find(|controller| controller.propagation_phase() == gtk::PropagationPhase::Capture)
            .unwrap();
        keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers])
    }

    fn change() -> ChangeSet {
        ChangeSet {
            item: eventix_state::actions::items::ItemId {
                calendar: "test".into(),
                uid: "event".into(),
                recurrence: None,
            },
            affected: vec![],
        }
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn editor_survives_main_window_browsing_shortcuts_popups_and_tray_hiding() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("org.eventix.EditorBrowsingTest")
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let (window, page) = crate::window::Window::test_new(&app);
        window.present();
        let editor = page.test_new_editor(options());
        let editor_window = page.imp().editor.window.borrow().clone().unwrap();
        drain();
        assert_eq!(editor_window.application(), Some(app));

        assert!(press(
            &editor_window,
            gdk::Key::Escape,
            gdk::ModifierType::empty()
        ));
        assert!(page.editor_open());
        assert!(!press(
            &window,
            gdk::Key::Return,
            gdk::ModifierType::CONTROL_MASK
        ));
        assert!(press(
            &editor_window,
            gdk::Key::Return,
            gdk::ModifierType::CONTROL_MASK
        ));
        assert!(page.editor_open(), "invalid saves leave the editor open");

        window.present();
        page.show_content(Some("2026-10-15".parse().unwrap()));
        gtk::prelude::GtkWindowExt::set_focus(&window, None::<&gtk::Widget>);
        assert!(press(&window, gdk::Key::Right, gdk::ModifierType::empty()));
        assert_eq!(
            page.imp().content.date.get(),
            Some("2026-10-22".parse().unwrap())
        );
        for (action, view) in [
            ("month-view", View::Month),
            ("list-view", View::List),
            ("week-view", View::Week),
        ] {
            gtk::prelude::ActionGroupExt::activate_action(&window, action, None);
            assert_eq!(page.imp().content.view.get(), view);
            assert!(page.editor_open());
        }
        page.focus_search();
        assert!(
            gtk::prelude::GtkWindowExt::focus(&window)
                .is_some_and(|focus| focus.is_ancestor(&*page.imp().search))
        );
        page.imp().search.set_text("meeting");
        page.imp().search.emit_activate();
        assert_eq!(page.imp().content.view.get(), View::Week);
        assert_eq!(page.imp().search.text(), "meeting");
        assert!(page.imp().list.get().unwrap().filter().keywords.is_empty());
        page.reload();
        assert!(page.editor_open());

        let details: EventDetails = glib::Object::new();
        details.set_child(Some(&gtk::Button::with_label("Other item")));
        page.imp()
            .details
            .show_details(page.upcast_ref(), page.upcast_ref(), details.clone());
        drain();
        assert!(press(
            &details,
            gdk::Key::Escape,
            gdk::ModifierType::empty()
        ));
        assert!(page.imp().details.current().is_none());
        assert!(page.editor_open());
        page.dismiss_popovers();
        assert!(Rc::ptr_eq(
            page.imp().editor.current.borrow().as_ref().unwrap(),
            &editor
        ));

        page.new_task();
        assert_eq!(
            page.imp().editor.window.borrow().as_ref(),
            Some(&editor_window)
        );
        window.hide_to_tray();
        drain();
        assert!(!editor_window.is_visible());
        assert!(page.editor_open());
        window.present_from_tray();
        drain();
        assert!(editor_window.is_visible());

        editor.set_saving(true);
        editor_window.close();
        window.close();
        assert!(page.editor_open());
        assert!(window.is_visible());
        editor.set_saving(false);
        editor_window.close();
        assert!(!page.editor_open());
        page.test_new_editor(options());
        let reopened = page.imp().editor.window.borrow().clone().unwrap();
        window.close();
        assert!(!page.editor_open());
        assert!(!reopened.is_visible());
        drain();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn editor_loads_and_saves_are_independent_of_popovers_and_reject_stale_responses() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let page: CalendarPage = glib::Object::new();
        let window = gtk::Window::builder().child(&page).build();
        window.present();
        let lifecycle = &page.imp().editor;
        let id = lifecycle.begin_request();
        page.show_content(Some("2026-10-22".parse().unwrap()));
        page.dismiss_popovers();
        assert!(lifecycle.loading.get());
        lifecycle.editor_loaded(&page, id + 1, Ok(options()));
        assert!(lifecycle.current.borrow().is_none());
        lifecycle.editor_loaded(&page, id, Ok(options()));
        let editor = lifecycle.current.borrow().clone().unwrap();
        let editor_window = lifecycle.window.borrow().clone().unwrap();
        drain();
        editor.set_saving(true);
        editor.cancel.emit_clicked();
        assert!(page.editor_open());
        assert!(lifecycle.saved(id, Err("Save conflict".into())).is_none());
        assert!(!editor.saving.get());
        assert!(editor_window.is_visible());
        assert!(Rc::ptr_eq(
            lifecycle.current.borrow().as_ref().unwrap(),
            &editor
        ));
        lifecycle.saved(id + 1, Ok(change()));
        assert!(page.editor_open());

        let details: EventDetails = glib::Object::new();
        details.set_child(Some(&gtk::Button::with_label("Other item")));
        page.imp()
            .details
            .show_details(page.upcast_ref(), page.upcast_ref(), details.clone());
        lifecycle.saved(id, Ok(change()));
        assert!(!page.editor_open());
        assert_eq!(page.imp().details.current(), Some(details));
        assert_eq!(
            page.imp().content.date.get(),
            Some("2026-10-22".parse().unwrap())
        );

        let id = lifecycle.begin_request();
        lifecycle.stop();
        lifecycle.editor_loaded(&page, id, Ok(options()));
        assert!(!page.editor_open());
        window.set_visible(false);
        lifecycle.present_editor(&page, ItemEditor::new(id + 2, options()));
        assert!(!lifecycle.window.borrow().as_ref().unwrap().is_visible());
        lifecycle.stop();
        assert!(lifecycle.window.borrow().is_none());
        window.close();
        drain();
    }
}
