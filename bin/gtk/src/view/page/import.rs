// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use gtk::{gio, glib, prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::{model::import::PreparedImport, view::import::ImportView};

#[derive(Default)]
pub(super) struct PageImports {
    files: RefCell<VecDeque<gio::File>>,
    active: Cell<bool>,
    generation: Cell<u64>,
    popup: RefCell<Option<Rc<ImportView>>>,
}

impl CalendarPage {
    pub(crate) fn open_imports(&self, files: &[gio::File]) {
        self.imp()
            .imports
            .files
            .borrow_mut()
            .extend(files.iter().cloned());
        self.imp().imports.next(self);
        if let Some(popup) = self.imp().imports.popup.borrow().as_ref() {
            popup.window.present();
        }
    }
}

impl PageImports {
    fn next(&self, page: &CalendarPage) {
        if self.active.get() {
            return;
        }
        let Some(file) = self.files.borrow_mut().pop_front() else {
            return;
        };
        self.active.set(true);
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        page.dismiss_popovers();
        let weak = page.downgrade();
        glib::MainContext::default().spawn_local(async move {
            let result = file.load_contents_future().await;
            let Some(page) = weak.upgrade() else {
                return;
            };
            if page.imp().imports.generation.get() != generation {
                return;
            }
            let result = result
                .map_err(|error| error.to_string())
                .and_then(|(bytes, _)| {
                    String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string())
                });
            match result {
                Ok(input) => {
                    if let Some(session) = page.imp().session.current.borrow().as_ref() {
                        session.prepare_import(input);
                    }
                }
                Err(error) => page.imp().imports.error(&page, &error),
            }
        });
    }

    pub fn prepared(&self, page: &CalendarPage, result: Result<PreparedImport, String>) {
        let preview = match result {
            Ok(preview) => preview,
            Err(error) => {
                self.error(page, &error);
                return;
            }
        };
        let Some(parent) = page.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let weak = page.downgrade();
        let closed_page = page.downgrade();
        let popup = ImportView::new(
            &parent,
            page.locale().as_ref(),
            &preview.clone(),
            move |calendar| {
                if let Some(page) = weak.upgrade()
                    && let Some(session) = page.imp().session.current.borrow().as_ref()
                {
                    session.import(preview.clone(), calendar);
                }
            },
            move || {
                // Defer advancing until GTK has completed the old window's close request.
                let weak = closed_page.clone();
                glib::idle_add_local_once(move || {
                    if let Some(page) = weak.upgrade() {
                        page.imp().imports.complete(&page);
                    }
                });
            },
        );
        popup.window.present();
        self.popup.replace(Some(popup));
    }

    pub fn finished(&self, page: &CalendarPage, result: Result<(), String>) {
        let popup = self.popup.borrow().clone();
        if let Some(popup) = popup {
            popup.finished(result, page.locale().as_ref());
        }
    }

    fn error(&self, page: &CalendarPage, error: &str) {
        let parent = page.root().and_downcast::<gtk::Window>();
        let dialog = gtk::AlertDialog::builder()
            .message(page.locale().translate("error.import_failed"))
            .detail(error)
            .modal(true)
            .build();
        let weak = page.downgrade();
        dialog.choose(parent.as_ref(), None::<&gio::Cancellable>, move |_| {
            if let Some(page) = weak.upgrade() {
                page.imp().imports.complete(&page);
            }
        });
    }

    fn complete(&self, page: &CalendarPage) {
        self.popup.take();
        self.active.set(false);
        self.next(page);
    }

    pub fn stop(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.files.borrow_mut().clear();
        if let Some(popup) = self.popup.take() {
            popup.window.destroy();
        }
        self.active.set(false);
    }
}
