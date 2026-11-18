// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use eventix_locale::Locale;
use eventix_state::auth::{AuthOutcome, AuthPrompt};
use gtk::{glib, prelude::*};

type Completion = Box<dyn FnOnce(bool)>;

struct Inner {
    window: gtk::Window,
    status: gtk::Label,
    code: gtk::Label,
    open: gtk::Button,
    url: RefCell<String>,
    finished: Cell<bool>,
    complete: RefCell<Option<Completion>>,
    locale: Arc<dyn Locale + Send + Sync>,
}

/// A modal popup displaying device-authentication prompts and outcomes.
pub(crate) struct SyncPopup(Rc<Inner>);

impl SyncPopup {
    pub(crate) fn new(
        parent: &gtk::Window,
        locale: Arc<dyn Locale + Send + Sync>,
        collection: String,
        complete: impl FnOnce(bool) + 'static,
    ) -> Self {
        let builder = gtk::Builder::from_resource(resource_path!("view/auth.ui"));
        let inner = Rc::new(Inner {
            window: builder.object("window").expect("auth window"),
            status: builder.object("status").expect("auth status"),
            code: builder.object("code").expect("auth code"),
            open: builder.object("open").expect("auth open button"),
            locale,
            url: RefCell::new(String::new()),
            finished: Cell::new(false),
            complete: RefCell::new(Some(Box::new(complete))),
        });
        inner.window.set_transient_for(Some(parent));
        inner.translate(&builder, &collection);
        inner.connect_actions(&builder);
        inner.window.present();
        Self(inner)
    }

    pub(crate) fn set_prompt(&self, prompt: AuthPrompt) {
        self.0.url.replace(prompt.verification_uri);
        self.0.code.set_text(&format!(
            "{} {}",
            self.0.locale.translate("Enter this code:"),
            prompt.user_code
        ));
        self.0.open.set_sensitive(true);
    }

    pub(crate) fn finished(&self, result: Result<AuthOutcome, String>) {
        match result {
            Ok(AuthOutcome::Complete) => self.0.finish(true),
            Ok(AuthOutcome::Declined) => self
                .0
                .status
                .set_text(self.0.locale.translate("Authentication was declined.")),
            Ok(AuthOutcome::Expired) => self.0.status.set_text(
                self.0
                    .locale
                    .translate("The authentication code expired. Please try again."),
            ),
            Err(error) => self.0.show_failure(&error),
        }
    }
}

impl Inner {
    fn translate(&self, builder: &gtk::Builder, collection: &str) {
        self.window
            .set_title(Some(self.locale.translate("Authentication Required")));
        self.status
            .set_text(self.locale.translate("Waiting for authentication..."));
        let explanation: gtk::Label = builder.object("explanation").expect("auth explanation");
        explanation.set_text(
            &formatx::formatx!(self.locale.translate("error.reauth_required"), collection).unwrap(),
        );
        for (id, text) in [
            ("open_label", "Open Microsoft sign-in"),
            ("cancel_label", "Cancel"),
        ] {
            let label: gtk::Label = builder.object(id).expect("auth button label");
            label.set_text(self.locale.translate(text));
        }
    }

    fn connect_actions(self: &Rc<Self>, builder: &gtk::Builder) {
        let weak = Rc::downgrade(self);
        self.open.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.open_browser();
            }
        });
        let weak = Rc::downgrade(self);
        let cancel: gtk::Button = builder.object("cancel").expect("auth cancel button");
        cancel.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.finish(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.finish(false);
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        let keys: gtk::EventControllerKey = builder.object("keys").expect("auth key controller");
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(inner) = weak.upgrade() {
                    inner.finish(false);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }

    fn open_browser(self: &Rc<Self>) {
        let launcher = gtk::UriLauncher::new(&self.url.borrow());
        let weak = Rc::downgrade(self);
        launcher.launch(
            Some(&self.window),
            None::<&gtk::gio::Cancellable>,
            move |result| {
                if let Err(error) = result
                    && let Some(inner) = weak.upgrade()
                {
                    inner.show_failure(&error.to_string());
                }
            },
        );
    }

    fn show_failure(&self, error: &str) {
        self.status.set_text(&format!(
            "{}\n{error}",
            self.locale
                .translate("Authentication failed. Please try again.")
        ));
    }

    fn finish(&self, authenticated: bool) {
        if self.finished.replace(true) {
            return;
        }
        self.window.close();
        if let Some(complete) = self.complete.take() {
            complete(authenticated);
        }
    }
}

impl Drop for SyncPopup {
    fn drop(&mut self) {
        self.0.finished.set(true);
        self.0.complete.take();
        self.0.window.close();
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use eventix_state::auth::{AuthOutcome, AuthPrompt};
    use gtk::prelude::*;

    use super::SyncPopup;

    #[test]
    #[ignore = "requires a GTK display"]
    fn authentication_popup_displays_code_and_closes_on_success_or_cancel() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let parent = gtk::Window::new();
        parent.present();
        let completed = Rc::new(Cell::new(None));
        let result = completed.clone();
        let popup = SyncPopup::new(
            &parent,
            eventix_locale::default(),
            "office".to_string(),
            move |success| result.set(Some(success)),
        );
        assert!(popup.0.window.is_modal());
        assert_eq!(popup.0.window.transient_for(), Some(parent.clone()));
        assert!(!popup.0.open.is_sensitive());
        popup.set_prompt(AuthPrompt {
            verification_uri: "https://microsoft.com/devicelogin".to_string(),
            user_code: "ABCD1234".to_string(),
        });
        assert!(popup.0.open.is_sensitive());
        assert!(popup.0.code.text().contains("ABCD1234"));
        assert_eq!(*popup.0.url.borrow(), "https://microsoft.com/devicelogin");
        popup.finished(Ok(AuthOutcome::Declined));
        assert_eq!(popup.0.status.text(), "Authentication was declined.");
        popup.finished(Ok(AuthOutcome::Expired));
        assert_eq!(
            popup.0.status.text(),
            "The authentication code expired. Please try again."
        );
        popup.finished(Ok(AuthOutcome::Complete));
        assert_eq!(completed.get(), Some(true));
        assert!(!popup.0.window.is_visible());
        popup.0.finish(false);
        assert_eq!(completed.get(), Some(true));
        drop(popup);

        completed.set(None);
        let result = completed.clone();
        let popup = SyncPopup::new(
            &parent,
            eventix_locale::default(),
            "office".to_string(),
            move |success| result.set(Some(success)),
        );
        popup.0.window.close();
        assert_eq!(completed.get(), Some(false));

        completed.set(None);
        let result = completed.clone();
        let popup = SyncPopup::new(
            &parent,
            eventix_locale::default(),
            "office".to_string(),
            move |success| result.set(Some(success)),
        );
        drop(popup);
        assert_eq!(completed.get(), None, "teardown must not retry sync");
        parent.close();
    }
}
