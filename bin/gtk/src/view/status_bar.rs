// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use eventix_locale::Locale;
use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    resource_template! { "view/status_bar.ui",
    pub struct StatusBar {
        #[template_child]
        pub status: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub sync_progress: gtk::TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub cancel_sync: gtk::TemplateChild<gtk::Button>,
        pub progress_timer: RefCell<Option<glib::SourceId>>,
    }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StatusBar {
        const NAME: &'static str = "EventixStatusBar";
        type Type = super::StatusBar;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for StatusBar {
        fn dispose(&self) {
            self.obj().stop();
            self.dispose_template();
        }
    }

    impl WidgetImpl for StatusBar {}
    impl BoxImpl for StatusBar {}
}

glib::wrapper! {
    pub struct StatusBar(ObjectSubclass<imp::StatusBar>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl StatusBar {
    pub fn set_text(&self, text: &str) {
        self.imp().status.set_text(text);
    }

    pub fn set_source(&self, source: &str) {
        self.imp().status.set_tooltip_text(Some(source));
    }

    pub fn set_locale(&self, locale: &dyn Locale) {
        let cancel = &self.imp().cancel_sync;
        cancel.set_tooltip_text(Some(locale.translate("Cancel synchronization")));
        cancel.update_property(&[gtk::accessible::Property::Label(
            locale.translate("Cancel synchronization"),
        )]);
    }

    pub fn connect_cancel(&self, callback: impl Fn() + 'static) {
        self.imp().cancel_sync.connect_clicked(move |_| callback());
    }

    pub fn can_cancel(&self) -> bool {
        self.imp().cancel_sync.is_sensitive()
    }

    pub fn cancelling(&self) {
        self.imp().cancel_sync.set_sensitive(false);
    }

    pub fn start_sync(&self, locale: &dyn Locale) {
        self.stop();
        let imp = self.imp();
        imp.sync_progress.set_fraction(0.0);
        imp.sync_progress.set_visible(true);
        imp.cancel_sync.set_sensitive(true);
        imp.cancel_sync.set_visible(true);
        imp.sync_progress
            .set_tooltip_text(Some(locale.translate("Synchronizing calendars…")));
    }

    pub fn set_progress(&self, completed: usize, total: usize) {
        self.imp().sync_progress.set_fraction(if total == 0 {
            1.0
        } else {
            completed as f64 / total as f64
        });
    }

    pub fn finish_sync(&self, completed: bool, cancelled: bool) {
        self.stop();
        if completed && !cancelled {
            self.imp().sync_progress.set_fraction(1.0);
            self.imp().sync_progress.set_visible(true);
        }
        if !cancelled {
            let weak = self.downgrade();
            self.imp()
                .progress_timer
                .replace(Some(glib::timeout_add_local_once(
                    std::time::Duration::from_secs(1),
                    move || {
                        if let Some(status) = weak.upgrade() {
                            status.imp().progress_timer.take();
                            status.imp().sync_progress.set_visible(false);
                        }
                    },
                )));
        }
    }

    pub fn stop(&self) {
        if let Some(timer) = self.imp().progress_timer.take() {
            timer.remove();
        }
        self.imp().sync_progress.set_visible(false);
        self.imp().cancel_sync.set_visible(false);
    }

    #[cfg(test)]
    pub fn text(&self) -> glib::GString {
        self.imp().status.text()
    }
}
