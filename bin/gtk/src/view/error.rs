// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::prelude::*;

#[derive(Clone)]
pub(super) struct ErrorMessage {
    pub widget: gtk::Box,
    pub prefix: gtk::Label,
    pub message: gtk::Label,
}

impl ErrorMessage {
    pub fn new() -> Self {
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .accessible_role(gtk::AccessibleRole::Alert)
            .focusable(true)
            .visible(false)
            .overflow(gtk::Overflow::Hidden)
            .build();
        widget.add_css_class("ev_management_error");
        let prefix = gtk::Label::new(None);
        prefix.add_css_class("ev_management_error_prefix");
        let message = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .max_width_chars(60)
            .hexpand(true)
            .build();
        message.add_css_class("ev_management_error_message");
        widget.append(&prefix);
        widget.append(&message);
        Self {
            widget,
            prefix,
            message,
        }
    }
}

pub(super) fn show_error(error: &ErrorMessage, message: &str, prefix: &str) {
    error.prefix.set_text(&format!("{prefix}:"));
    error.message.set_text(message);
    error.widget.set_visible(true);
}

/// A single error box whose appearance and changing message height are animated.
pub(super) struct AnimatedError {
    pub widget: gtk::Revealer,
    stack: gtk::Stack,
}

impl AnimatedError {
    pub fn new() -> Self {
        let stack = gtk::Stack::builder()
            .hhomogeneous(false)
            .vhomogeneous(false)
            .interpolate_size(true)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(200)
            .build();
        stack.connect_transition_running_notify(Self::clean_stack);
        let widget = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .transition_duration(200)
            .child(&stack)
            .build();
        Self { widget, stack }
    }

    pub fn show(&self, message: &str, prefix: &str) -> gtk::Label {
        let error = ErrorMessage::new();
        error.widget.add_css_class("ev_editor_error");
        show_error(&error, message, prefix);
        self.stack.add_child(&error.widget);
        self.stack.set_visible_child(&error.widget);
        Self::clean_stack(&self.stack);
        self.widget.set_reveal_child(true);
        error.message
    }

    pub fn clear(&self) {
        self.widget.set_reveal_child(false);
    }

    fn clean_stack(stack: &gtk::Stack) {
        if !stack.is_transition_running() {
            let mut child = stack.first_child();
            while let Some(page) = child {
                child = page.next_sibling();
                if Some(&page) != stack.visible_child().as_ref() {
                    stack.remove(&page);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use gtk::prelude::*;

    use super::AnimatedError;

    #[test]
    #[ignore = "requires a GTK display"]
    fn bottom_error_box_animates_show_message_height_changes_and_hide() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let error = AnimatedError::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&gtk::Label::new(Some("Editor")));
        content.append(&error.widget);
        let window = gtk::Window::builder()
            .default_width(600)
            .child(&content)
            .build();
        window.settings().set_gtk_enable_animations(true);
        let samples = Rc::new(RefCell::new(Vec::new()));
        let observed = samples.clone();
        error.widget.add_tick_callback(move |widget, _| {
            observed
                .borrow_mut()
                .push(widget.measure(gtk::Orientation::Vertical, 600).1);
            gtk::glib::ControlFlow::Continue
        });
        let settle = || {
            for _ in 0..35 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        window.present();
        settle();
        assert_eq!(error.widget.measure(gtk::Orientation::Vertical, 600).1, 0);
        let assert_animated = |from: i32, to: i32| {
            assert_ne!(from, to);
            assert!(
                samples
                    .borrow()
                    .iter()
                    .any(|height| (from.min(to) + 1..from.max(to)).contains(height)),
                "height changes include intermediate frames: {:?}",
                samples.borrow()
            );
        };
        samples.borrow_mut().clear();
        error.show("Short <error> & details", "Error");
        settle();
        let short = error.widget.measure(gtk::Orientation::Vertical, 600).1;
        assert_animated(0, short);
        samples.borrow_mut().clear();
        let message = error.show(&"A long error with details. ".repeat(40), "Fehler");
        settle();
        let long = error.widget.measure(gtk::Orientation::Vertical, 600).1;
        assert!(long > short);
        assert_animated(short, long);
        let page = message.parent().unwrap();
        let prefix = page.first_child().unwrap();
        assert_eq!(prefix.height(), message.height());
        assert_eq!(error.stack.pages().n_items(), 1);
        samples.borrow_mut().clear();
        error.show("Short error", "Error");
        settle();
        assert_eq!(
            error.widget.measure(gtk::Orientation::Vertical, 600).1,
            short
        );
        assert_animated(long, short);
        samples.borrow_mut().clear();
        error.clear();
        settle();
        assert_animated(short, 0);
        assert_eq!(error.widget.measure(gtk::Orientation::Vertical, 600).1, 0);
        window.settings().set_gtk_enable_animations(false);
        for message in ["First", "Second", "Third"] {
            error.show(message, "Error");
            assert_eq!(error.stack.pages().n_items(), 1);
        }
        error.clear();
        window.close();
    }
}
