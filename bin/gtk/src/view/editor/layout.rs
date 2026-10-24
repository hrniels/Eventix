// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_locale::Locale;
use gtk::prelude::*;

/// A full-width form row whose header swaps its summary for optional inline controls.
pub(super) struct FormSection {
    pub toggle: gtk::ToggleButton,
    pub preview: gtk::Label,
    error: gtk::Image,
}

impl FormSection {
    pub fn new(
        parent: &gtk::Box,
        locale: &dyn Locale,
        name: &str,
        child: &impl IsA<gtk::Widget>,
        controls: Option<&gtk::Widget>,
    ) -> Self {
        let builder = gtk::Builder::from_resource(resource_path!("view/editor/section.ui"));
        let widget: gtk::Box = builder.object("section").expect("editor section");
        let caption_slot: gtk::Box = builder
            .object("caption_slot")
            .expect("section caption slot");
        caption_slot.append(&caption(locale, name));
        let content: gtk::Box = builder.object("content").expect("section content");
        content.append(child);

        let controls_slot: gtk::Box = builder
            .object("controls_slot")
            .expect("section controls slot");
        if let Some(controls) = controls {
            controls_slot.append(controls);
        }

        let section = Self {
            toggle: builder.object("toggle").expect("section toggle"),
            preview: builder.object("preview").expect("section preview"),
            error: builder.object("error").expect("section error icon"),
        };
        section.connect_toggle(
            builder.object("arrow").expect("section expansion hint"),
            builder.object("revealer").expect("section revealer"),
            controls.map(|_| controls_slot.upcast()),
        );
        parent.append(&widget);

        section
    }

    pub fn set_invalid(&self, invalid: bool) {
        self.error.set_visible(invalid);
        self.toggle
            .update_state(&[gtk::accessible::State::Invalid(if invalid {
                gtk::AccessibleInvalidState::True
            } else {
                gtk::AccessibleInvalidState::False
            })]);
    }

    fn connect_toggle(
        &self,
        arrow: gtk::Label,
        content: gtk::Revealer,
        controls: Option<gtk::Widget>,
    ) {
        let summary = self.preview.clone();
        self.toggle.connect_toggled(move |button| {
            let expanded = button.is_active();

            summary.set_visible(!expanded);
            arrow.set_visible(!expanded);
            content.set_reveal_child(expanded);
            button.set_hexpand(!expanded || controls.is_none());
            if let Some(controls) = &controls {
                controls.set_visible(expanded);
            }
        });
    }
}

fn caption(locale: &dyn Locale, name: &str) -> gtk::Box {
    let builder = gtk::Builder::from_resource(resource_path!("view/editor/caption.ui"));
    let widget = builder.object("caption").expect("editor caption");
    let icon = match name {
        "Calendar" => "collection",
        "Summary" => "card-heading",
        "Location" => "geo-alt-fill",
        "Date" => "calendar-date",
        "Reminder" => "alarm",
        "Description" => "journal-text",
        "Repeat" => "arrow-clockwise",
        "Attendees" => "people-fill",
        _ => "journal-text",
    };
    builder
        .object::<gtk::Image>("icon")
        .expect("editor caption icon")
        .set_resource(Some(&format!(
            "{}/icons/{icon}-symbolic.svg",
            crate::RESOURCE_PREFIX
        )));
    builder
        .object::<gtk::Label>("label")
        .expect("editor caption label")
        .set_text(locale.translate(name));

    widget
}

pub(super) fn row(
    parent: &gtk::Box,
    locale: &dyn Locale,
    name: &str,
    child: &impl IsA<gtk::Widget>,
) {
    let builder = gtk::Builder::from_resource(resource_path!("view/editor/field_row.ui"));
    let widget: gtk::Box = builder.object("row").expect("editor field row");
    widget.append(&caption(locale, name));
    child.set_hexpand(true);
    widget.append(child);
    parent.append(&widget);
}

pub(super) fn body_label(locale: &dyn Locale, text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(locale.translate(text)));
    label.add_css_class("ev_editor_body_label");
    label.set_xalign(0.0);

    label
}

pub(super) fn image(name: &str) -> gtk::Image {
    let icon = gtk::Image::from_resource(&format!(
        "{}/icons/{name}-symbolic.svg",
        crate::RESOURCE_PREFIX
    ));
    icon.set_pixel_size(19);

    icon
}

/// Binds grouped native toggles to the same selection model used by draft conversion.
pub(super) fn segments(locale: &dyn Locale, model: &gtk::DropDown, labels: &[&str]) -> gtk::Box {
    let widget = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    widget.add_css_class("ev_editor_segments");
    let mut first = None;
    for (index, label) in labels.iter().enumerate() {
        let button = gtk::ToggleButton::with_label(locale.translate(label));
        button.set_group(first.as_ref());
        if first.is_none() {
            first = Some(button.clone());
        }
        button.set_active(model.selected() == index as u32);
        bind_segment(&button, model, index as u32);
        widget.append(&button);
    }

    widget
}

fn bind_segment(button: &gtk::ToggleButton, model: &gtk::DropDown, index: u32) {
    let selection = model.downgrade();
    button.connect_toggled(move |button| {
        if button.is_active()
            && let Some(model) = selection.upgrade()
        {
            model.set_selected(index);
        }
    });

    let weak = button.downgrade();
    model.connect_selected_notify(move |model| {
        if let Some(button) = weak.upgrade() {
            button.set_active(model.selected() == index);
        }
    });
}

pub(super) fn choices(
    locale: &dyn Locale,
    model: &gtk::DropDown,
    labels: &[&str],
) -> Vec<gtk::CheckButton> {
    let mut buttons = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        let button = gtk::CheckButton::with_label(locale.translate(label));
        button.set_group(buttons.first());
        button.set_active(model.selected() == index as u32);
        bind_choice(&button, model, index as u32);
        buttons.push(button);
    }

    buttons
}

pub(super) fn bind_choice(button: &gtk::CheckButton, model: &gtk::DropDown, index: u32) {
    let selection = model.downgrade();
    button.connect_toggled(move |button| {
        if button.is_active()
            && let Some(model) = selection.upgrade()
        {
            model.set_selected(index);
        }
    });

    let weak = button.downgrade();
    model.connect_selected_notify(move |model| {
        if let Some(button) = weak.upgrade() {
            button.set_active(model.selected() == index);
        }
    });
}

#[cfg(test)]
mod tests {
    use gtk::{graphene, prelude::*};

    use super::FormSection;

    #[test]
    #[ignore = "requires a GTK display"]
    fn section_preview_keeps_the_button_as_pointer_target_across_toggles() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let locale = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::English,
            chrono_tz::UTC,
        )
        .unwrap();
        let form = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let section = FormSection::new(
            &form,
            locale.as_ref(),
            "Description",
            &gtk::TextView::new(),
            None,
        );
        section.preview.set_text("Event description");
        let window = gtk::Window::builder()
            .child(&form)
            .default_width(600)
            .build();
        window.present();
        let drain = || {
            for _ in 0..10 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();
        let point = section
            .preview
            .compute_point(
                &section.toggle,
                &graphene::Point::new(
                    section.preview.width() as f32 / 2.0,
                    section.preview.height() as f32 / 2.0,
                ),
            )
            .unwrap();

        for expanded in [false, true, false, true, false] {
            section.toggle.set_active(expanded);
            drain();
            assert_eq!(
                section.toggle.pick(
                    f64::from(point.x()),
                    f64::from(point.y()),
                    gtk::PickFlags::DEFAULT,
                ),
                Some(section.toggle.clone().upcast()),
                "the same pointer position must target the toggle, expanded={expanded}",
            );
        }
        window.close();
        drain();
    }
}
