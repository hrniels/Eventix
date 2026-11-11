// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::Cell, rc::Rc};

use eventix_locale::{Locale, LocaleType};
use eventix_state::Preferences;
use gtk::{glib, prelude::*};

/// Owns a modal preferences editor until it is saved or cancelled.
pub(crate) struct SettingsPopup {
    window: gtk::Window,
    language: gtk::DropDown,
    interval: gtk::SpinButton,
    delay: gtk::SpinButton,
    save: gtk::Button,
    cancel: gtk::Button,
    error: gtk::Label,
    failure_title: String,
    saving: Cell<bool>,
}

impl SettingsPopup {
    pub fn new(
        parent: &gtk::Window,
        locale: &dyn Locale,
        preferences: Preferences,
        submit: impl Fn(Preferences) -> bool + 'static,
    ) -> Rc<Self> {
        let builder = gtk::Builder::from_resource(resource_path!("view/settings.ui"));
        let window: gtk::Window = builder.object("window").unwrap();
        let content: gtk::Box = builder.object("content").unwrap();
        window.set_transient_for(Some(parent));
        window.set_title(Some(locale.translate("Settings")));
        let language: gtk::DropDown = builder.object("language").unwrap();
        language.set_model(Some(&gtk::StringList::new(&[
            locale.translate("English"),
            locale.translate("German"),
        ])));
        language.set_selected(u32::from(preferences.locale_type == LocaleType::German));
        language.set_factory(Some(&language_factory()));
        language.set_list_factory(Some(&language_factory()));
        let interval: gtk::SpinButton = builder.object("interval").unwrap();
        interval.set_value(f64::from(preferences.sync_interval_minutes));
        let delay: gtk::SpinButton = builder.object("delay").unwrap();
        delay.set_value(f64::from(preferences.post_delay_seconds));
        for (id, text, widget) in [
            (
                "language_label",
                "Language",
                language.upcast_ref::<gtk::Widget>(),
            ),
            ("interval_label", "Sync interval", interval.upcast_ref()),
            ("delay_label", "Upload delay", delay.upcast_ref()),
        ] {
            let label: gtk::Label = builder.object(id).unwrap();
            label.set_text(locale.translate(text));
            label.set_mnemonic_widget(Some(widget));
            widget.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
        }
        for (id, text, widget) in [
            ("interval_unit", "minutes", &interval),
            ("delay_unit", "seconds", &delay),
        ] {
            let label: gtk::Label = builder.object(id).unwrap();
            label.set_text(locale.translate(text));
            widget.update_relation(&[gtk::accessible::Relation::DescribedBy(
                &[label.upcast_ref()],
            )]);
        }
        for (id, text, widget) in [
            (
                "interval_help",
                "Time between automatic calendar synchronizations. Local changes are uploaded separately after the upload delay.",
                &interval,
            ),
            (
                "delay_help",
                "Wait after the latest edit before uploading. Zero uploads immediately.",
                &delay,
            ),
        ] {
            let help = locale.translate(text);
            let help_icon: gtk::Image = builder.object(id).unwrap();
            help_icon.set_tooltip_text(Some(help));
            widget.update_property(&[gtk::accessible::Property::Description(help)]);
        }
        let error = gtk::Label::new(None);
        error.set_wrap(true);
        error.set_max_width_chars(45);
        error.set_xalign(0.0);
        error.set_visible(false);
        error.add_css_class("error");
        error.set_margin_start(6);
        error.set_margin_end(6);
        content.append(&error);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_margin_start(6);
        buttons.set_margin_end(6);
        buttons.set_halign(gtk::Align::End);
        let cancel = action_button(
            locale.translate("Cancel"),
            resource_path!("icons/x-circle-symbolic.svg"),
        );
        let save = action_button(
            locale.translate("Save"),
            resource_path!("icons/floppy2-fill-symbolic.svg"),
        );
        buttons.append(&save);
        buttons.append(&cancel);
        content.append(&buttons);
        let popup = Rc::new(Self {
            window,
            language,
            interval,
            delay,
            save,
            cancel,
            error,
            failure_title: locale.translate("Could not save settings").into(),
            saving: Cell::new(false),
        });
        popup.connect_actions(submit);
        popup.present();
        popup
    }

    fn connect_actions(self: &Rc<Self>, submit: impl Fn(Preferences) -> bool + 'static) {
        let popup = self;
        let weak = Rc::downgrade(popup);
        popup.save.connect_clicked(move |_| {
            if let Some(popup) = weak.upgrade() {
                popup.interval.update();
                popup.delay.update();
                popup.set_saving(true);
                if !submit(popup.preferences()) {
                    popup.set_saving(false);
                    popup.error.set_text(&popup.failure_title);
                    popup.error.set_visible(true);
                }
            }
        });
        let weak = Rc::downgrade(popup);
        popup.cancel.connect_clicked(move |_| {
            if let Some(popup) = weak.upgrade() {
                popup.window.close();
            }
        });
        let weak = Rc::downgrade(popup);
        popup.window.connect_close_request(move |_| {
            if weak.upgrade().is_some_and(|popup| popup.saving.get()) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(popup);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(popup) = weak.upgrade() {
                    popup.window.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        popup.window.add_controller(keys);
    }

    fn preferences(&self) -> Preferences {
        Preferences {
            locale_type: if self.language.selected() == 0 {
                LocaleType::English
            } else {
                LocaleType::German
            },
            sync_interval_minutes: self.interval.value_as_int() as u32,
            post_delay_seconds: self.delay.value_as_int() as u32,
        }
    }

    fn set_saving(&self, saving: bool) {
        self.saving.set(saving);
        for widget in [
            self.language.upcast_ref::<gtk::Widget>(),
            self.interval.upcast_ref(),
            self.delay.upcast_ref(),
            self.save.upcast_ref(),
            self.cancel.upcast_ref(),
        ] {
            widget.set_sensitive(!saving);
        }
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn is_open(&self) -> bool {
        self.window.is_visible()
    }

    pub fn finished(&self, result: &Result<Preferences, String>) {
        self.set_saving(false);
        match result {
            Ok(_) => self.window.close(),
            Err(error) => {
                self.error
                    .set_text(&format!("{}\n{}", self.failure_title, error));
                self.error.set_visible(true);
            }
        }
    }
}

impl Drop for SettingsPopup {
    fn drop(&mut self) {
        self.window.destroy();
    }
}

fn action_button(label: &str, resource: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.set_halign(gtk::Align::Center);
    let image = gtk::Image::from_resource(resource);
    image.set_pixel_size(16);
    content.append(&image);
    content.append(&gtk::Label::new(Some(label)));
    gtk::Button::builder()
        .child(&content)
        .css_classes(["ev_editor_action"])
        .build()
}

/// Displays the same flag and translated language name in the selection and dropdown list.
fn language_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let icon = gtk::Image::new();
        icon.set_pixel_size(16);
        row.append(&icon);
        row.append(&gtk::Label::new(None));
        item.set_child(Some(&row));
    });
    factory.connect_bind(|_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let language = item.item().and_downcast::<gtk::StringObject>().unwrap();
        let row = item.child().and_downcast::<gtk::Box>().unwrap();
        let icon = row.first_child().and_downcast::<gtk::Image>().unwrap();
        icon.set_resource(Some(match item.position() {
            0 => resource_path!("icons/language-en.svg"),
            _ => resource_path!("icons/language-de.svg"),
        }));
        let label = row.last_child().and_downcast::<gtk::Label>().unwrap();
        label.set_text(&language.string());
    });
    factory
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use eventix_locale::LocaleType;
    use eventix_state::Preferences;
    use gtk::prelude::*;

    use super::SettingsPopup;

    #[test]
    #[ignore = "requires a GTK display"]
    fn settings_popup_saves_a_draft_and_keeps_failed_saves_editable() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let parent = gtk::Window::new();
        parent.present();
        let saved = Rc::new(Cell::new(None));
        let captured = saved.clone();
        let popup = SettingsPopup::new(
            &parent,
            eventix_locale::default().as_ref(),
            Preferences::default(),
            move |preferences| {
                captured.set(Some(preferences));
                true
            },
        );
        assert!(popup.window.is_modal());
        assert_eq!(popup.window.transient_for(), Some(parent.clone()));
        assert_eq!(popup.preferences(), Preferences::default());
        for (button, label, resource) in [
            (
                &popup.cancel,
                "Cancel",
                resource_path!("icons/x-circle-symbolic.svg"),
            ),
            (
                &popup.save,
                "Save",
                resource_path!("icons/floppy2-fill-symbolic.svg"),
            ),
        ] {
            assert!(button.has_css_class("ev_editor_action"));
            let content = button.child().and_downcast::<gtk::Box>().unwrap();
            assert_eq!(content.halign(), gtk::Align::Center);
            let image = content.first_child().and_downcast::<gtk::Image>().unwrap();
            assert_eq!(image.resource().as_deref(), Some(resource));
            assert_eq!(image.pixel_size(), 16);
            let text = content.last_child().and_downcast::<gtk::Label>().unwrap();
            assert_eq!(text.text(), label);
        }
        assert_eq!(popup.interval.adjustment().lower(), 1.0);
        assert_eq!(popup.interval.adjustment().upper(), 1440.0);
        assert_eq!(popup.delay.adjustment().lower(), 0.0);
        assert_eq!(popup.delay.adjustment().upper(), 86_400.0);
        assert_eq!(
            popup.interval.measure(gtk::Orientation::Horizontal, -1),
            popup.delay.measure(gtk::Orientation::Horizontal, -1)
        );
        popup.delay.set_value(86_400.0);
        assert_eq!(popup.preferences().post_delay_seconds, 86_400);
        let content = popup.window.child().unwrap();
        assert_eq!(content.margin_start(), 0);
        assert_eq!(content.margin_end(), 0);
        assert!(popup.language.factory().is_some());
        assert!(popup.language.list_factory().is_some());
        let fields = content.first_child().and_downcast::<gtk::Box>().unwrap();
        let mut row = fields.first_child().unwrap();
        for (field, unit) in [
            ("Language", None),
            ("Sync interval", Some("minutes")),
            ("Upload delay", Some("seconds")),
        ] {
            assert!(row.has_css_class("ev_details_row"));
            assert!(row.has_css_class("ev_settings_row"));
            assert_eq!(row.clone().downcast::<gtk::Box>().unwrap().spacing(), 24);
            let heading = row.first_child().unwrap();
            let label = heading
                .clone()
                .downcast::<gtk::Label>()
                .unwrap_or_else(|heading| {
                    heading.first_child().and_downcast::<gtk::Label>().unwrap()
                });
            assert_eq!(label.text(), field);
            assert!(label.has_css_class("ev_details_label"));
            if field != "Language" {
                let help = label.next_sibling().and_downcast::<gtk::Image>().unwrap();
                assert_eq!(
                    help.icon_name().as_deref(),
                    Some("dialog-question-symbolic")
                );
                assert_eq!(
                    help.tooltip_text().as_deref(),
                    Some(if field == "Sync interval" {
                        "Time between automatic calendar synchronizations. Local changes are uploaded separately after the upload delay."
                    } else {
                        "Wait after the latest edit before uploading. Zero uploads immediately."
                    })
                );
                assert!(help.next_sibling().is_none());
            } else {
                assert!(heading.is::<gtk::Label>());
            }
            if let Some(unit) = unit {
                let controls = row.last_child().unwrap();
                assert!(controls.first_child().unwrap().is::<gtk::SpinButton>());
                let label = controls.last_child().and_downcast::<gtk::Label>().unwrap();
                assert_eq!(label.text(), unit);
            }
            if let Some(next) = row.next_sibling() {
                row = next;
            }
        }
        assert!(fields.next_sibling().and_downcast::<gtk::Label>().unwrap() == popup.error);
        popup.language.set_selected(1);
        popup.interval.set_value(30.0);
        popup.delay.set_value(0.0);
        assert!(saved.get().is_none());
        popup.save.emit_clicked();
        assert_eq!(
            saved.get(),
            Some(Preferences {
                locale_type: LocaleType::German,
                sync_interval_minutes: 30,
                post_delay_seconds: 0,
            })
        );
        assert!(!popup.save.is_sensitive());
        popup.finished(&Err("test failure".into()));
        assert!(popup.is_open());
        assert!(popup.save.is_sensitive());
        assert!(popup.error.is_visible());
        popup.finished(&Ok(saved.get().unwrap()));
        assert!(!popup.is_open());
        parent.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn settings_popup_cancel_discards_the_draft() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let parent = gtk::Window::new();
        parent.present();
        let popup = SettingsPopup::new(
            &parent,
            eventix_locale::default().as_ref(),
            Preferences::default(),
            |_| panic!("cancel must not save"),
        );
        popup.interval.set_value(60.0);
        popup.cancel.emit_clicked();
        assert!(!popup.is_open());
        parent.close();
    }
}
