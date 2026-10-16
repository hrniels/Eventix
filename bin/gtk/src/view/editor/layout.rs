// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_locale::Locale;
use eventix_view::editor::EditorCalendar;
use gtk::{gdk, prelude::*};

/// A full-width form row whose header swaps its summary for optional inline controls.
pub(super) struct FormSection {
    pub toggle: gtk::ToggleButton,
    pub preview: gtk::Label,
}

impl FormSection {
    pub fn new(
        parent: &gtk::Box,
        locale: &dyn Locale,
        name: &str,
        child: &impl IsA<gtk::Widget>,
        controls: Option<&gtk::Widget>,
    ) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.add_css_class("ev_editor_row");
        let (section, arrow) = Self::create_heading(locale, name);
        let header = Self::create_header(&section.toggle, controls);
        widget.append(&header);
        let content = Self::create_content(child);
        widget.append(&content);
        section.connect_toggle(arrow, content, controls.cloned());
        parent.append(&widget);
        section
    }

    fn create_heading(locale: &dyn Locale, name: &str) -> (Self, gtk::Label) {
        let toggle = gtk::ToggleButton::new();
        toggle.add_css_class("ev_editor_section_toggle");
        toggle.set_cursor_from_name(Some("pointer"));
        toggle.set_hexpand(true);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        heading.append(&caption(locale, name));
        let preview = gtk::Label::new(None);
        preview.add_css_class("ev_editor_preview");
        preview.set_hexpand(true);
        preview.set_xalign(0.0);
        preview.set_ellipsize(gtk::pango::EllipsizeMode::End);
        preview.set_width_chars(1);
        heading.append(&preview);
        let arrow = gtk::Label::new(Some("▸"));
        arrow.add_css_class("ev_editor_expand_hint");
        heading.append(&arrow);
        toggle.set_child(Some(&heading));
        (Self { toggle, preview }, arrow)
    }

    fn create_header(toggle: &gtk::ToggleButton, controls: Option<&gtk::Widget>) -> gtk::Box {
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("ev_editor_row_header");
        header.append(toggle);
        if let Some(controls) = controls {
            controls.set_visible(false);
            controls.set_halign(gtk::Align::End);
            controls.set_hexpand(true);
            header.append(controls);
        }
        header
    }

    fn create_content(child: &impl IsA<gtk::Widget>) -> gtk::Revealer {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("ev_editor_expanded");
        content.append(child);
        gtk::Revealer::builder()
            .child(&content)
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .transition_duration(300)
            .build()
    }

    fn connect_toggle(&self, arrow: gtk::Label, content: gtk::Revealer, controls: Option<gtk::Widget>) {
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
    let widget = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    widget.add_css_class("ev_editor_caption");
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
    widget.append(&image(icon));
    let label = gtk::Label::new(Some(locale.translate(name)));
    label.set_xalign(0.0);
    label.add_css_class("ev_editor_field_label");
    widget.append(&label);
    widget
}

pub(super) fn row(
    parent: &gtk::Box,
    locale: &dyn Locale,
    name: &str,
    child: &impl IsA<gtk::Widget>,
) {
    let widget = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    widget.add_css_class("ev_editor_row");
    widget.add_css_class("ev_editor_field_row");
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
        "/io/github/hrniels/Eventix/gtk/icons/{name}-symbolic.svg"
    ));
    icon.set_pixel_size(19);
    icon
}

pub(super) fn action_button(locale: &dyn Locale, text: &str, icon: &str) -> gtk::Button {
    let button = gtk::Button::new();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.set_halign(gtk::Align::Center);
    let icon = image(icon);
    icon.set_pixel_size(16);
    content.append(&icon);
    content.append(&gtk::Label::new(Some(locale.translate(text))));
    button.set_child(Some(&content));
    button.add_css_class("ev_editor_action");
    button
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

fn bind_choice(button: &gtk::CheckButton, model: &gtk::DropDown, index: u32) {
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

pub(super) fn calendar_factory(calendars: &[EditorCalendar]) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        setup_calendar_item(item);
    });
    let colors: Vec<_> = calendars
        .iter()
        .map(|calendar| calendar.background.clone())
        .collect();
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        bind_calendar_item(item, &colors);
    });
    factory
}

fn setup_calendar_item(item: &gtk::ListItem) {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let dot = gtk::DrawingArea::new();
    dot.set_size_request(18, 18);
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    content.append(&dot);
    content.append(&label);
    item.set_child(Some(&content));
}

fn bind_calendar_item(item: &gtk::ListItem, colors: &[String]) {
    let Some(content) = item.child() else {
        return;
    };
    if let Some(label) = content.last_child().and_downcast::<gtk::Label>()
        && let Some(text) = item.item().and_downcast::<gtk::StringObject>()
    {
        label.set_text(&text.string());
    }
    if let Some(dot) = content.first_child().and_downcast::<gtk::DrawingArea>() {
        let color = colors
            .get(item.position() as usize)
            .and_then(|color| gdk::RGBA::parse(color).ok())
            .unwrap_or(gdk::RGBA::TRANSPARENT);
        set_calendar_dot_color(&dot, color);
    }
}

fn set_calendar_dot_color(dot: &gtk::DrawingArea, color: gdk::RGBA) {
    dot.set_draw_func(move |_, cr, width, height| {
        cr.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        cr.arc(
            f64::from(width) / 2.0,
            f64::from(height) / 2.0,
            f64::from(width.min(height)) / 2.0 - 1.0,
            0.0,
            std::f64::consts::TAU,
        );
        let _ = cr.fill();
    });
}
