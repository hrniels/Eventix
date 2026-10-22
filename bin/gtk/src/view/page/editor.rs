// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use eventix_ical::objects::CalCompType;
use eventix_state::actions::items::{EditScope, ItemId, UpdateItem};
use eventix_view::editor::EditorOptions;
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

use super::CalendarPage;
use super::create::EventSlot;
use crate::model::session::{ChangeSet, SavedItem};
use crate::view::details::EventDetails;
use crate::view::editor::{ItemEditor, has_nested_popup, is_save_key};

/// Owns editor requests and the active editor's lifecycle.
#[derive(Default)]
pub(super) struct PageEditor {
    pub(super) current: RefCell<Option<Rc<ItemEditor>>>,
    request: Cell<u64>,
    loading: Cell<bool>,
    slot: Cell<Option<EventSlot>>,
}

impl PageEditor {
    pub(super) fn edit_item(&self, page: &CalendarPage, item: ItemId, scope: EditScope) {
        if self.editor_open() {
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

        // Opening a new popup closes the previous one and invalidates pending editor loads.
        // Install the new editor only after that cleanup has completed.
        let shown = if editor.existing.borrow().is_some() {
            page.imp().details.show_editor(&editor.popup_panel())
        } else {
            page.imp()
                .details
                .show_new_editor(page.upcast_ref(), &editor.popup_panel());
            true
        };
        if shown {
            self.current.replace(Some(editor.clone()));
            editor.focus();
        }
    }

    pub(super) fn discard_editor(&self) {
        self.current.take();
        self.loading.set(false);
        self.slot.set(None);
        self.request.set(self.request.get().wrapping_add(1));
    }

    pub(super) fn editor_open(&self) -> bool {
        self.loading.get() || self.current.borrow().is_some()
    }

    /// Handles editor shortcuts before either the window or a native popup consumes them.
    pub(super) fn handle_editor_key(
        &self,
        page: &CalendarPage,
        key: gdk::Key,
        modifiers: gdk::ModifierType,
    ) -> Option<glib::Propagation> {
        if !self.editor_open() {
            return None;
        }

        if key == gdk::Key::Escape
            && page
                .root()
                .and_downcast::<gtk::Window>()
                .is_some_and(|window| !has_nested_popup(&window))
        {
            self.finish_editor(page, None);
            return Some(glib::Propagation::Stop);
        }

        if is_save_key(key, modifiers) {
            self.save_editor(page);
            return Some(glib::Propagation::Stop);
        }

        Some(glib::Propagation::Proceed)
    }

    pub(super) fn new_item(&self, page: &CalendarPage, kind: CalCompType) {
        self.new_item_at(page, kind, None);
    }

    pub(super) fn new_event_at(&self, page: &CalendarPage, slot: EventSlot) {
        self.new_item_at(page, CalCompType::Event, Some(slot));
    }

    fn new_item_at(&self, page: &CalendarPage, kind: CalCompType, slot: Option<EventSlot>) {
        if self.editor_open() {
            return;
        }

        if let Some(session) = page.imp().session.current.borrow().as_ref() {
            page.dismiss_popovers();
            let id = self.begin_request();
            self.slot.set(slot);
            session.prepare_editor(id, kind);
        }
    }

    pub(super) fn finish_editor(&self, page: &CalendarPage, item: Option<&SavedItem>) {
        let editor = self.current.borrow().clone();
        if editor.as_ref().is_some_and(|editor| editor.saving.get()) {
            return;
        }

        if let Some(editor) = self.current.take() {
            if editor.existing.borrow().is_some() {
                let locale = page.locale();
                let updated = item.map(|item| match item {
                    SavedItem::Event(event) => EventDetails::new(event, locale.as_ref()),
                    SavedItem::Task(task) => EventDetails::new_sidebar(task, locale.as_ref()),
                });
                page.imp().details.restore_details(updated);
            } else {
                page.imp().details.close();
                page.imp()
                    .sidebar
                    .new_item_button(editor.kind())
                    .grab_focus();
            }
        }

        self.loading.set(false);
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
            Err(issues) => editor.show_validation_errors(&issues),
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
        match result {
            Ok((options, edit)) => self.present_existing_editor(page, id, options, *edit),
            Err(error) => page.show_error("Could not apply changes", &error),
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
        match result {
            Ok(mut options) => {
                let slot = self.slot.take();
                if let Some(slot) = slot {
                    options.start = slot.start;
                }
                let editor = if slot.is_some_and(|slot| slot.all_day) {
                    ItemEditor::new_at(id, options, true)
                } else {
                    ItemEditor::new(id, options)
                };
                self.present_editor(page, editor);
            }
            Err(error) => page.show_error("Could not load local calendars", &error),
        }
    }

    pub(super) fn saved(
        &self,
        page: &CalendarPage,
        id: u64,
        result: Result<(ChangeSet, Option<SavedItem>), String>,
    ) -> Option<ChangeSet> {
        let editor = self
            .current
            .borrow()
            .clone()
            .filter(|editor| editor.id == id);

        match result {
            Ok((change, item)) => {
                if let Some(editor) = editor {
                    editor.set_saving(false);
                    self.finish_editor(page, item.as_ref());
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

    pub(super) fn stop(&self, page: &CalendarPage) {
        if let Some(editor) = self.current.borrow().as_ref() {
            editor.set_saving(false);
        }

        self.finish_editor(page, None);
    }
}
