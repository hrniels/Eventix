// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use eventix_locale::Locale;
use eventix_state::{EmailAccount, SyncTimeBound, SyncerType};
use eventix_view::calendars::{Backend, Collection};
use gtk::prelude::*;

use super::{ActionHandler, action, clear, icon_action};
use crate::model::management::{CollectionOperation, Operation, Request};

pub(super) struct CollectionActions {
    pub widget: gtk::Box,
    pub editor: RefCell<Option<gtk::Window>>,
    model: RefCell<Collection>,
    locale: RefCell<Arc<dyn Locale + Send + Sync>>,
    on_action: ActionHandler,
    pending: Cell<bool>,
    draft: RefCell<Option<CollectionForm>>,
}

impl CollectionActions {
    pub(super) fn new(
        model: &Collection,
        locale: Arc<dyn Locale + Send + Sync>,
        on_action: &ActionHandler,
    ) -> Rc<Self> {
        let controls = Rc::new(Self {
            widget: gtk::Box::new(gtk::Orientation::Horizontal, 4),
            editor: RefCell::default(),
            model: RefCell::new(model.clone()),
            locale: RefCell::new(locale),
            on_action: on_action.clone(),
            pending: Cell::new(false),
            draft: RefCell::default(),
        });
        controls.widget.set_halign(gtk::Align::Start);
        controls.buttons();
        controls
    }

    pub(super) fn update(
        self: &Rc<Self>,
        model: &Collection,
        locale: Arc<dyn Locale + Send + Sync>,
    ) {
        let old = self.model.replace(model.clone());
        let translated =
            self.locale.borrow().translate("Calendars") != locale.translate("Calendars");
        self.locale.replace(locale);
        if translated
            || old.backend != model.backend
            || old.reload != model.reload
            || old.discover != model.discover
        {
            self.buttons();
        }
    }

    fn buttons(self: &Rc<Self>) {
        clear(&self.widget);
        let locale = self.locale.borrow();
        let model = self.model.borrow();
        let delete = icon_action("user-trash-symbolic", "Delete", locale.as_ref());
        delete.add_css_class("ev_button_critical");
        self.connect_operation(&delete, CollectionOperation::Delete);
        self.widget.append(&delete);

        let edit = icon_action("pencil-symbolic", "Edit", locale.as_ref());
        let weak = Rc::downgrade(self);
        edit.connect_clicked(move |_| {
            if let Some(controls) = weak.upgrade() {
                controls.open();
            }
        });
        self.widget.append(&edit);
        for (visible, icon, label, operation) in [
            (
                model.reload,
                "view-refresh-symbolic",
                "Reload",
                CollectionOperation::Reload,
            ),
            (
                model.discover,
                "find-location-symbolic",
                "Discover",
                CollectionOperation::Discover,
            ),
            (
                true,
                "view-refresh-symbolic",
                "Sync",
                CollectionOperation::Sync,
            ),
        ] {
            if visible {
                let button = action(icon, label, locale.as_ref());
                if matches!(operation, CollectionOperation::Reload) {
                    button.add_css_class("ev_button_warn");
                }
                self.connect_operation(&button, operation);
                self.widget.append(&button);
            }
        }
        if model.backend != Backend::FileSystem {
            let log = action("view-list-symbolic", "Log", locale.as_ref());
            let weak = Rc::downgrade(self);
            log.connect_clicked(move |_| {
                if let Some(controls) = weak.upgrade() {
                    controls.log();
                }
            });
            self.widget.append(&log);
        }
    }

    fn connect_operation(self: &Rc<Self>, button: &gtk::Button, operation: CollectionOperation) {
        let weak = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(controls) = weak.upgrade() {
                controls.confirm(operation.clone());
            }
        });
    }

    fn confirm(self: &Rc<Self>, operation: CollectionOperation) {
        let warning = match operation {
            CollectionOperation::Delete => "warning.delete_collection",
            CollectionOperation::Reload => "warning.reload_collection",
            _ => {
                self.submit(operation);
                return;
            }
        };
        let locale = self.locale.borrow();
        let dialog = gtk::AlertDialog::builder()
            .message(locale.translate("Are you sure?"))
            .detail(locale.translate(warning))
            .buttons(vec![
                locale.translate("Cancel"),
                locale.translate(if matches!(operation, CollectionOperation::Delete) {
                    "Delete"
                } else {
                    "Reload"
                }),
            ])
            .cancel_button(0)
            .default_button(0)
            .build();
        let weak = Rc::downgrade(self);
        dialog.choose(
            self.widget.root().and_downcast::<gtk::Window>().as_ref(),
            None::<&gtk::gio::Cancellable>,
            move |result| {
                if result == Ok(1)
                    && let Some(controls) = weak.upgrade()
                {
                    controls.submit(operation);
                }
            },
        );
    }

    fn submit(&self, operation: CollectionOperation) {
        let callback = self.on_action.borrow();
        let Some(callback) = callback.as_ref() else {
            return;
        };
        if self.pending.replace(true) {
            return;
        }
        self.widget.set_sensitive(false);
        if let Some(editor) = self.editor.borrow().as_ref() {
            editor.set_sensitive(false);
        }
        callback(Request {
            collection: self.model.borrow().id.clone(),
            calendar: String::new(),
            folder: String::new(),
            configured: true,
            operation: Operation::Collection(operation),
        });
    }

    pub(super) fn finish(&self, request: &Request, success: bool) {
        if !self.pending.replace(false) {
            return;
        }
        self.widget.set_sensitive(true);
        if let Some(editor) = self.editor.borrow().as_ref() {
            editor.set_sensitive(true);
        }
        if success
            && matches!(
                request.operation,
                Operation::Collection(CollectionOperation::Save { .. })
            )
        {
            self.close();
        }
    }

    fn close(&self) {
        if let Some(editor) = self.editor.take() {
            editor.destroy();
        }
        if let Some(draft) = self.draft.take() {
            draft.password.set_text("");
        }
    }

    fn open(self: &Rc<Self>) {
        if let Some(editor) = self.editor.borrow().as_ref() {
            editor.present();
            return;
        }
        let locale = self.locale.borrow();
        let model = self.model.borrow();
        let form = CollectionForm::new(&model.syncer, locale.as_ref());
        form.identity(&model.id, &model.syncer, locale.as_ref());
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        buttons.set_halign(gtk::Align::End);
        let cancel = action("window-close-symbolic", "Cancel", locale.as_ref());
        let weak = Rc::downgrade(self);
        cancel.connect_clicked(move |_| {
            if let Some(controls) = weak.upgrade() {
                controls.close();
            }
        });
        let save = action("document-save-symbolic", "Save", locale.as_ref());
        let weak = Rc::downgrade(self);
        save.connect_clicked(move |_| {
            if let Some(controls) = weak.upgrade() {
                controls.save();
            }
        });
        buttons.append(&save);
        buttons.append(&cancel);
        form.widget.append(&buttons);
        let editor = self.editor_window(&form.widget, locale.as_ref());
        self.draft.replace(Some(form));
        self.editor.replace(Some(editor.clone()));
        if let Some(form) = self.draft.borrow().as_ref() {
            if matches!(self.model.borrow().syncer, SyncerType::FileSystem { .. }) {
                gtk::prelude::GtkWindowExt::set_focus(&editor, Some(&form.path));
            } else {
                gtk::prelude::GtkWindowExt::set_focus(&editor, Some(&form.name));
            }
        }
        editor.present();
    }

    fn save(&self) {
        if self.pending.get() {
            return;
        }
        let result = self.draft.borrow().as_ref().map(|draft| {
            draft.error.set_visible(false);
            draft.operation(&self.model.borrow().syncer, self.locale.borrow().as_ref())
        });
        match result {
            Some(Ok(operation)) => self.submit(operation),
            Some(Err(error)) => self.error(&error.to_string()),
            None => {}
        }
    }

    fn editor_window(self: &Rc<Self>, content: &gtk::Box, locale: &dyn Locale) -> gtk::Window {
        content.set_margin_top(8);
        content.set_margin_bottom(8);
        content.set_margin_start(8);
        content.set_margin_end(8);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(600)
            .child(content)
            .build();
        let editor = gtk::Window::builder()
            .title(locale.translate("Edit Collection"))
            .default_width(640)
            .modal(true)
            .destroy_with_parent(true)
            .child(&scroll)
            .build();
        editor.set_transient_for(self.widget.root().and_downcast::<gtk::Window>().as_ref());
        let weak = Rc::downgrade(self);
        editor.connect_close_request(move |_| {
            if let Some(controls) = weak.upgrade()
                && !controls.pending.get()
            {
                controls.close();
            }
            gtk::glib::Propagation::Stop
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter)
                && modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK)
            {
                if let Some(controls) = weak.upgrade() {
                    controls.save();
                }
                return gtk::glib::Propagation::Stop;
            }
            if key == gtk::gdk::Key::Escape {
                if let Some(controls) = weak.upgrade()
                    && !controls.pending.get()
                {
                    controls.close();
                }
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        editor.add_controller(keys);
        editor
    }

    pub(super) fn error(&self, message: &str) {
        if let Some(form) = self.draft.borrow().as_ref() {
            form.error.set_text(&format!(
                "{}: {message}",
                self.locale.borrow().translate("Error")
            ));
            form.error.set_visible(true);
            form.error.grab_focus();
            return;
        }
        gtk::AlertDialog::builder()
            .message(self.locale.borrow().translate("Could not apply changes"))
            .detail(message)
            .build()
            .show(
                self.editor.borrow().as_ref().or(self
                    .widget
                    .root()
                    .and_downcast::<gtk::Window>()
                    .as_ref()),
            );
    }

    fn log(self: &Rc<Self>) {
        let file = gtk::gio::File::for_path(&self.model.borrow().log_path);
        let weak = Rc::downgrade(self);
        gtk::glib::MainContext::default().spawn_local(async move {
            let result = file.load_contents_future().await;
            let Some(controls) = weak.upgrade() else {
                return;
            };
            let locale = controls.locale.borrow();
            let content = match result {
                Ok((bytes, _)) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(error) if error.matches(gtk::gio::IOErrorEnum::NotFound) => {
                    locale.translate("No entries").to_owned()
                }
                Err(error) => {
                    controls.error(&error.to_string());
                    return;
                }
            };
            let title = formatx::formatx!(
                locale.translate("Log of collection '{}'"),
                &controls.model.borrow().id
            )
            .unwrap();
            let buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
            buffer.set_text(&content);
            let text = gtk::TextView::builder()
                .buffer(&buffer)
                .editable(false)
                .cursor_visible(false)
                .monospace(true)
                .left_margin(12)
                .right_margin(12)
                .build();
            let scroll = gtk::ScrolledWindow::builder()
                .child(&text)
                .vexpand(true)
                .hexpand(true)
                .build();
            let window = gtk::Window::builder()
                .title(&title)
                .default_width(800)
                .default_height(500)
                .child(&scroll)
                .modal(true)
                .build();
            window.set_transient_for(
                controls
                    .widget
                    .root()
                    .and_downcast::<gtk::Window>()
                    .as_ref(),
            );
            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            let weak = window.downgrade();
            keys.connect_key_pressed(move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape {
                    if let Some(window) = weak.upgrade() {
                        window.close();
                    }
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            window.add_controller(keys);
            window.present();
        });
    }
}

impl Drop for CollectionActions {
    fn drop(&mut self) {
        self.close();
    }
}

struct CollectionForm {
    widget: gtk::Box,
    syncer_section: gtk::Box,
    labels: gtk::SizeGroup,
    error: gtk::Label,
    path: gtk::Entry,
    name: gtk::Entry,
    email: gtk::Entry,
    url: gtk::Entry,
    username: gtk::Entry,
    password: gtk::PasswordEntry,
    read_only: gtk::CheckButton,
    unlimited: gtk::CheckButton,
    limited: gtk::CheckButton,
    years: gtk::SpinButton,
}

impl CollectionForm {
    fn new(syncer: &SyncerType, locale: &dyn Locale) -> Self {
        let form = Self {
            widget: gtk::Box::new(gtk::Orientation::Vertical, 6),
            syncer_section: gtk::Box::new(gtk::Orientation::Vertical, 6),
            labels: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
            error: gtk::Label::builder()
                .xalign(0.0)
                .wrap(true)
                .max_width_chars(60)
                .focusable(true)
                .accessible_role(gtk::AccessibleRole::Alert)
                .visible(false)
                .build(),
            path: gtk::Entry::new(),
            name: gtk::Entry::new(),
            email: gtk::Entry::new(),
            url: gtk::Entry::new(),
            username: gtk::Entry::new(),
            password: gtk::PasswordEntry::builder().show_peek_icon(true).build(),
            read_only: gtk::CheckButton::new(),
            unlimited: gtk::CheckButton::with_label(locale.translate("Unlimited")),
            limited: gtk::CheckButton::with_label(locale.translate("Last N years")),
            years: gtk::SpinButton::with_range(1.0, 100.0, 1.0),
        };
        form.syncer_section
            .add_css_class("ev_management_hover_area");
        form.widget.append(&form.syncer_section);
        for check in [&form.read_only, &form.unlimited, &form.limited] {
            check.add_css_class("ev_management_check");
            check.set_halign(gtk::Align::Start);
            check.set_valign(gtk::Align::Center);
        }
        if let SyncerType::FileSystem { path } = syncer {
            form.path.set_text(path);
            let field = gtk::Box::new(gtk::Orientation::Vertical, 4);
            field.append(&form.path);
            field.append(&form.explanation("info.collection_folder_path", locale));
            form.row("Path", true, &field, None, locale);
            form.path.set_tooltip_text(Some(locale.translate("Path")));
            form.path.update_property(&[
                gtk::accessible::Property::Label(locale.translate("Path")),
                gtk::accessible::Property::Description(
                    locale.translate("info.collection_folder_path"),
                ),
                gtk::accessible::Property::Required(true),
            ]);
            form.footer(locale);
            return form;
        }
        form.remote_fields(syncer, locale);
        form.footer(locale);
        form
    }

    fn remote_fields(&self, syncer: &SyncerType, locale: &dyn Locale) {
        if let Some(email) = syncer.email() {
            self.name.set_text(email.name());
            self.email.set_text(email.org_address());
        }
        self.row("Your Name", true, &self.name, None, locale);
        let help = if matches!(syncer, SyncerType::O365 { .. }) {
            "info.name_and_email_o365"
        } else {
            "info.name_and_email"
        };
        self.row("Email Address", true, &self.email, Some(help), locale);
        self.separator();
        if let SyncerType::VDirSyncer { url, username, .. } = syncer {
            self.url.set_text(url);
            self.username
                .set_text(username.as_deref().unwrap_or_default());
            self.row("Location", true, &self.url, None, locale);
        }
        self.read_only.set_active(syncer.is_read_only());
        self.row("Read-only", false, &self.read_only, None, locale);
        self.sync_window(syncer, locale);
        if matches!(syncer, SyncerType::VDirSyncer { .. }) {
            self.separator();
            self.row("Username", false, &self.username, None, locale);
            self.row(
                "Password",
                false,
                &self.password,
                Some("info.edit_password_help"),
                locale,
            );
        }
    }

    fn sync_window(&self, syncer: &SyncerType, locale: &dyn Locale) {
        let span = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        self.limited.set_group(Some(&self.unlimited));
        span.append(&self.unlimited);
        span.append(&self.limited);
        span.append(&self.years);
        span.append(&gtk::Label::new(Some(locale.translate("year(s)"))));
        self.years.set_width_chars(3);
        self.years.set_numeric(true);
        self.years
            .update_property(&[gtk::accessible::Property::Label(
                locale.translate("Last N years"),
            )]);
        self.row("Sync window", false, &span, None, locale);
        let years = match syncer.time_span().map(|span| &span.start) {
            Some(SyncTimeBound::Years(years)) => Some(*years),
            _ => None,
        };
        self.limited.set_active(years.is_some());
        self.unlimited.set_active(years.is_none());
        self.years.set_value(f64::from(years.unwrap_or(5)));
        self.years.set_sensitive(years.is_some());
        let spinner = self.years.downgrade();
        self.limited.connect_toggled(move |check| {
            if let Some(spinner) = spinner.upgrade() {
                spinner.set_sensitive(check.is_active());
            }
        });
    }

    fn identity(&self, id: &str, syncer: &SyncerType, locale: &dyn Locale) {
        let identity = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let name = gtk::Label::builder()
            .label(id)
            .selectable(true)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .max_width_chars(50)
            .build();
        let name_row = self.field_row("Name", false, &name, None, locale);
        name_row.add_css_class("ev_management_hover_area");
        identity.append(&name_row);
        let modes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        modes.add_css_class("linked");
        modes.add_css_class("ev_management_sync_types");
        modes.set_halign(gtk::Align::End);
        let mut first = None;
        for (title, selected) in [
            ("CalDAV", matches!(syncer, SyncerType::VDirSyncer { .. })),
            ("O365", matches!(syncer, SyncerType::O365 { .. })),
            (
                "FileSystem",
                matches!(syncer, SyncerType::FileSystem { .. }),
            ),
        ] {
            let button = gtk::ToggleButton::with_label(locale.translate(title));
            button.set_group(first.as_ref());
            if first.is_none() {
                first = Some(button.clone());
            }
            button.set_active(selected);
            button.set_sensitive(selected);
            modes.append(&button);
        }
        self.syncer_section
            .prepend(&self.field_row("Syncer", false, &modes, None, locale));
        self.widget.prepend(&identity);
    }

    fn separator(&self) {
        let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        separator.add_css_class("ev_management_form_separator");
        self.syncer_section.append(&separator);
    }

    fn explanation(&self, key: &str, locale: &dyn Locale) -> gtk::Label {
        gtk::Label::builder()
            .label(locale.translate(key))
            .xalign(0.0)
            .wrap(true)
            .max_width_chars(50)
            .build()
    }

    fn footer(&self, locale: &dyn Locale) {
        let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        separator.add_css_class("ev_management_form_separator");
        self.widget.append(&separator);
        self.widget
            .append(&self.explanation("info.required_fields", locale));
        self.error.add_css_class("ev_management_error");
        self.widget.append(&self.error);
    }

    fn row(
        &self,
        title: &str,
        required: bool,
        input: &impl IsA<gtk::Widget>,
        help: Option<&str>,
        locale: &dyn Locale,
    ) {
        self.syncer_section
            .append(&self.field_row(title, required, input, help, locale));
    }

    fn field_row(
        &self,
        title: &str,
        required: bool,
        input: &impl IsA<gtk::Widget>,
        help: Option<&str>,
        locale: &dyn Locale,
    ) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        heading.set_valign(gtk::Align::Center);
        let label = gtk::Label::builder()
            .label(if required {
                format!("{} *", locale.translate(title))
            } else {
                locale.translate(title).to_owned()
            })
            .xalign(0.0)
            .build();
        label.set_mnemonic_widget(Some(input));
        label.add_css_class("ev_management_form_label");
        heading.append(&label);
        if let Some(help) = help {
            let image = gtk::Image::from_icon_name("dialog-question-symbolic");
            image.set_tooltip_text(Some(locale.translate(help)));
            heading.append(&image);
            input
                .as_ref()
                .update_property(&[gtk::accessible::Property::Description(
                    locale.translate(help),
                )]);
        }
        self.labels.add_widget(&heading);
        if matches!(title, "Name" | "Syncer") {
            let icon = if title == "Name" {
                gtk::Image::from_resource(resource_path!("icons/collection-symbolic.svg"))
            } else {
                gtk::Image::from_icon_name("view-refresh-symbolic")
            };
            icon.set_pixel_size(16);
            let leading = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            leading.append(&icon);
            leading.append(&heading);
            row.append(&leading);
            row.add_css_class("ev_management_main_row");
        } else {
            // Reserve the main rows' 16-pixel icon and 6-pixel gap without adding field icons.
            row.set_margin_start(22);
            row.add_css_class("ev_management_sync_row");
            row.append(&heading);
        }
        input.set_hexpand(true);
        input.set_tooltip_text(Some(locale.translate(title)));
        input.as_ref().update_property(&[
            gtk::accessible::Property::Label(locale.translate(title)),
            gtk::accessible::Property::Required(required),
        ]);
        row.append(input);
        row
    }

    fn operation(
        &self,
        current: &SyncerType,
        locale: &dyn Locale,
    ) -> anyhow::Result<CollectionOperation> {
        let mut syncer = current.clone();
        if let SyncerType::FileSystem { path } = &mut syncer {
            *path = self.path.text().to_string();
            if path.is_empty() {
                anyhow::bail!(locale.translate("error.collection_path").to_owned());
            }
            if !std::path::Path::new(path).is_dir() {
                anyhow::bail!(locale.translate("error.collection_existing_dir").to_owned());
            }
        } else {
            if self.name.text().is_empty() {
                anyhow::bail!(locale.translate("error.collection_your_name").to_owned());
            }
            if !email_address::EmailAddress::is_valid(&self.email.text()) {
                anyhow::bail!(locale.translate("error.collection_your_email").to_owned());
            }
            let years = if self.limited.is_active() {
                Some(
                    self.years
                        .text()
                        .parse::<u32>()
                        .ok()
                        .filter(|years| (1..=100).contains(years))
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                locale
                                    .translate("error.collection_time_span_years_range")
                                    .to_owned()
                            )
                        })?,
                )
            } else {
                None
            };
            if let SyncerType::VDirSyncer { url, username, .. } = &mut syncer {
                let value = self.url.text();
                if !gtk::glib::Uri::parse(&value, gtk::glib::UriFlags::NONE).is_ok_and(|uri| {
                    matches!(uri.scheme().as_str(), "http" | "https") && uri.host().is_some()
                }) {
                    anyhow::bail!(
                        formatx::formatx!(locale.translate("error.collection_location"), value)
                            .unwrap()
                    );
                }
                *url = value.to_string();
                *username =
                    (!self.username.text().is_empty()).then(|| self.username.text().to_string());
            }
            if let SyncerType::VDirSyncer {
                email,
                read_only,
                time_span,
                ..
            }
            | SyncerType::O365 {
                email,
                read_only,
                time_span,
                ..
            } = &mut syncer
            {
                *email =
                    EmailAccount::new(self.name.text().to_string(), self.email.text().to_string());
                *read_only = self.read_only.is_active();
                time_span.start = years.map_or(SyncTimeBound::Infinite, SyncTimeBound::Years);
            }
        }
        Ok(CollectionOperation::Save {
            syncer: Box::new(syncer),
            password: (!self.password.text().is_empty()).then(|| self.password.text().to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use eventix_state::{EmailAccount, EncryptedPassword, SyncTimeBound, SyncTimeSpan, SyncerType};
    use gtk::prelude::*;

    use super::CollectionForm;
    use crate::model::management::CollectionOperation;

    fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut result = vec![widget.clone()];
        let mut child = widget.first_child();
        while let Some(widget) = child {
            result.extend(descendants(&widget));
            child = widget.next_sibling();
        }
        result
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn collection_forms_match_html_identity_help_required_fields_and_sync_choices() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let locale = eventix_locale::default();
        let email = EmailAccount::new("User".into(), "user@example.com".into());
        for (backend, syncer, help) in [
            (
                "FileSystem",
                SyncerType::FileSystem {
                    path: "/calendars".into(),
                },
                "info.collection_folder_path",
            ),
            (
                "CalDAV",
                SyncerType::VDirSyncer {
                    email: email.clone(),
                    url: "https://example.com".into(),
                    read_only: true,
                    username: Some("user".into()),
                    password: None,
                    time_span: Default::default(),
                },
                "info.name_and_email",
            ),
            (
                "O365",
                SyncerType::O365 {
                    email: email.clone(),
                    read_only: false,
                    password: EncryptedPassword::default(),
                    time_span: Default::default(),
                },
                "info.name_and_email_o365",
            ),
        ] {
            let form = CollectionForm::new(&syncer, locale.as_ref());
            form.identity("collection-id", &syncer, locale.as_ref());
            let widgets = descendants(form.widget.upcast_ref());
            let labels: Vec<_> = widgets
                .iter()
                .filter_map(|widget| widget.downcast_ref::<gtk::Label>())
                .map(|label| label.text())
                .collect();
            for expected in [
                "Name",
                "collection-id",
                "Syncer",
                "CalDAV",
                "O365",
                "FileSystem",
                locale.translate("info.required_fields"),
            ] {
                assert!(
                    labels.iter().any(|label| label == expected),
                    "{backend}: {expected}"
                );
            }
            let buttons: Vec<_> = widgets
                .iter()
                .filter_map(|widget| widget.downcast_ref::<gtk::ToggleButton>())
                .collect();
            assert_eq!(buttons.len(), 3);
            for button in buttons {
                let selected = button.label().as_deref() == Some(backend);
                assert_eq!(button.is_active(), selected);
                assert_eq!(button.is_sensitive(), selected);
            }
            if backend == "FileSystem" {
                assert!(labels.iter().any(|label| label == "Path *"));
                assert!(labels.iter().any(|label| label == locale.translate(help)));
                assert!(form.password.parent().is_none());
            } else {
                for required in ["Your Name *", "Email Address *"] {
                    assert!(labels.iter().any(|label| label == required));
                }
                assert!(widgets.iter().any(|widget| widget.is::<gtk::Image>()
                    && widget.tooltip_text().as_deref() == Some(locale.translate(help))));
                for choice in ["Unlimited", "Last N years"] {
                    assert!(
                        widgets
                            .iter()
                            .filter_map(|widget| widget.downcast_ref::<gtk::CheckButton>())
                            .any(|check| check.label().as_deref() == Some(choice))
                    );
                }
                assert!(labels.iter().any(|label| label == "year(s)"));
                assert_eq!(form.years.adjustment().lower(), 1.0);
                assert_eq!(form.years.adjustment().upper(), 100.0);
                assert!(form.unlimited.is_active());
                assert!(!form.years.is_sensitive());
                form.limited.set_active(true);
                assert!(!form.unlimited.is_active());
                assert!(form.years.is_sensitive());
                form.unlimited.set_active(true);
                assert!(!form.limited.is_active());
                assert!(!form.years.is_sensitive());
                if backend == "CalDAV" {
                    assert!(labels.iter().any(|label| label == "Location *"));
                    assert!(form.password.text().is_empty());
                    assert!(widgets.iter().any(|widget| widget.is::<gtk::Image>()
                        && widget.tooltip_text().as_deref()
                            == Some(locale.translate("info.edit_password_help"))));
                } else {
                    assert!(form.password.parent().is_none());
                    assert!(form.url.parent().is_none());
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(dir.path(), dir.path());
        let german = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::German,
            chrono_tz::UTC,
        )
        .unwrap();
        let syncer = SyncerType::O365 {
            email,
            read_only: false,
            password: EncryptedPassword::default(),
            time_span: Default::default(),
        };
        let form = CollectionForm::new(&syncer, german.as_ref());
        form.identity("collection-id", &syncer, german.as_ref());
        let widgets = descendants(form.widget.upcast_ref());
        for key in ["Syncer", "info.required_fields", "year(s)"] {
            assert!(
                widgets
                    .iter()
                    .filter_map(|widget| widget.downcast_ref::<gtk::Label>())
                    .any(|label| label.text() == german.translate(key))
            );
        }
        assert_eq!(form.unlimited.label().as_deref(), Some("Unbegrenzt"));
        form.limited.set_active(true);
        form.years.set_text("101");
        assert_eq!(
            form.operation(&syncer, german.as_ref())
                .unwrap_err()
                .to_string(),
            german.translate("error.collection_time_span_years_range")
        );
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn collection_forms_validate_backends_and_preserve_hidden_settings() {
        gtk::init().unwrap();
        let locale = eventix_locale::default();
        let dir = tempfile::tempdir().unwrap();
        let local = SyncerType::FileSystem {
            path: dir.path().to_string_lossy().into_owned(),
        };
        let form = CollectionForm::new(&local, locale.as_ref());
        form.path.set_text("");
        assert!(form.operation(&local, locale.as_ref()).is_err());
        form.path
            .set_text(&dir.path().join("missing").to_string_lossy());
        assert!(form.operation(&local, locale.as_ref()).is_err());
        form.path.set_text(&dir.path().to_string_lossy());
        assert!(form.operation(&local, locale.as_ref()).is_ok());

        let remote = SyncerType::VDirSyncer {
            email: EmailAccount::new("User".into(), "user@example.com".into()),
            url: "https://example.com".into(),
            read_only: true,
            username: None,
            password: None,
            time_span: Default::default(),
        };
        let form = CollectionForm::new(&remote, locale.as_ref());
        assert!(form.read_only.is_active());
        form.email.set_text("invalid");
        assert!(form.operation(&remote, locale.as_ref()).is_err());
        form.email.set_text("new@example.com");
        form.url.set_text("not a URL");
        assert!(form.operation(&remote, locale.as_ref()).is_err());
        form.url.set_text("https://example.com/updated");
        form.username.set_text("new-user");
        assert!(form.operation(&remote, locale.as_ref()).is_ok());
        form.password.set_text("new-secret");
        form.limited.set_active(true);
        form.years.set_value(3.0);
        form.years.set_text("101");
        assert!(form.operation(&remote, locale.as_ref()).is_err());
        form.years.set_text("0");
        assert!(form.operation(&remote, locale.as_ref()).is_err());
        form.years.set_text("invalid");
        assert!(form.operation(&remote, locale.as_ref()).is_err());
        form.years.set_value(3.0);
        form.years.set_text("3");
        let operation = form.operation(&remote, locale.as_ref()).unwrap();
        assert!(!format!("{operation:?}").contains("new-secret"));
        let CollectionOperation::Save { syncer, password } = operation else {
            panic!("expected save");
        };
        assert_eq!(password.as_deref(), Some("new-secret"));
        assert_eq!(syncer.email().unwrap().org_address(), "new@example.com");
        assert_eq!(syncer.time_span().unwrap().start, SyncTimeBound::Years(3));

        let o365 = SyncerType::O365 {
            email: EmailAccount::new("User".into(), "user@example.com".into()),
            read_only: false,
            password: EncryptedPassword {
                nonce: "nonce".into(),
                ciphertext: "encrypted".into(),
            },
            time_span: SyncTimeSpan {
                start: SyncTimeBound::Years(7),
                end: SyncTimeBound::Years(2),
            },
        };
        let form = CollectionForm::new(&o365, locale.as_ref());
        assert_eq!(form.years.value_as_int(), 7);
        form.limited.set_active(false);
        let CollectionOperation::Save { syncer, password } =
            form.operation(&o365, locale.as_ref()).unwrap()
        else {
            panic!("expected save");
        };
        assert_eq!(syncer.password(), o365.password());
        assert!(password.is_none());
        assert_eq!(syncer.time_span().unwrap().start, SyncTimeBound::Infinite);
        assert_eq!(syncer.time_span().unwrap().end, SyncTimeBound::Years(2));
    }
}
