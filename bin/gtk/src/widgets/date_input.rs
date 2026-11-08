// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::NaiveDate;
use eventix_locale::Locale;
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

use super::DatePicker;

mod imp {
    use super::*;

    pub struct DateInput {
        pub entry: gtk::Entry,
        pub calendar: DatePicker,
        pub popup: gtk::Popover,
    }

    impl Default for DateInput {
        fn default() -> Self {
            Self {
                entry: gtk::Entry::new(),
                calendar: glib::Object::new(),
                popup: gtk::Popover::new(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DateInput {
        const NAME: &'static str = "EventixDateInput";
        type Type = super::DateInput;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for DateInput {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            self.entry.set_width_chars(10);
            self.entry.set_hexpand(true);
            obj.append(&self.entry);
            self.calendar.set_focus_on_map(false);
            // Neither the calendar nor its navigation buttons belong to the form's tab order.
            self.popup.set_can_focus(false);
            self.popup.set_autohide(false);
            self.popup.set_child(Some(&self.calendar));
            self.popup.set_parent(&*obj);
            obj.connect_input();
        }

        fn dispose(&self) {
            self.popup.unparent();
        }
    }

    impl WidgetImpl for DateInput {}
    impl BoxImpl for DateInput {}
}

glib::wrapper! {
    /// An editable date field whose calendar popup leaves typing focus in the field.
    pub struct DateInput(ObjectSubclass<imp::DateInput>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl DateInput {
    /// Creates a localized date field and calendar.
    pub(crate) fn new(locale: &dyn Locale, date: NaiveDate) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().calendar.set_locale(locale, chrono::Weekday::Mon);
        obj.imp().calendar.select_day(date);
        obj.imp().entry.set_text(&date.to_string());
        obj.imp()
            .entry
            .set_tooltip_text(Some(locale.translate("Choose date")));
        obj
    }

    /// Returns the editable field used to read and validate the date.
    pub(crate) fn entry(&self) -> gtk::Entry {
        self.imp().entry.clone()
    }

    /// Returns the calendar used for explicit date selection.
    pub(crate) fn calendar(&self) -> DatePicker {
        self.imp().calendar.clone()
    }

    fn open(&self) {
        let imp = self.imp();
        if !imp.entry.is_sensitive() {
            return;
        }
        if let Ok(date) = NaiveDate::parse_from_str(&imp.entry.text(), "%Y-%m-%d") {
            imp.calendar.select_day(date);
        }
        imp.popup.set_visible(false);
        imp.popup.popup();
    }

    fn connect_input(&self) {
        let imp = self.imp();
        let weak = self.downgrade();
        imp.entry.connect_changed(move |entry| {
            if let Some(obj) = weak.upgrade()
                && let Ok(date) = NaiveDate::parse_from_str(&entry.text(), "%Y-%m-%d")
            {
                obj.imp().calendar.select_day(date);
            }
        });
        let weak = self.downgrade();
        imp.calendar.connect_day_adjusted(move |calendar| {
            if let Some(obj) = weak.upgrade() {
                obj.imp().entry.set_text(&calendar.date().to_string());
            }
        });
        let weak = self.downgrade();
        imp.calendar.connect_day_selected(move |calendar| {
            if let Some(obj) = weak.upgrade() {
                let date = calendar.date();
                obj.imp().entry.grab_focus();
                obj.imp().entry.set_text(&date.to_string());
                obj.imp().popup.popdown();
            }
        });
        let focus = gtk::EventControllerFocus::new();
        let weak = self.downgrade();
        focus.connect_enter(move |_| {
            if let Some(obj) = weak.upgrade() {
                obj.open();
            }
        });
        let weak = self.downgrade();
        focus.connect_leave(move |_| {
            if let Some(obj) = weak.upgrade() {
                obj.imp().popup.popdown();
            }
        });
        // Calendar buttons may temporarily take focus while being clicked.
        self.add_controller(focus);
        let weak = self.downgrade();
        imp.entry.connect_unmap(move |_| {
            if let Some(obj) = weak.upgrade() {
                obj.imp().popup.popdown();
            }
        });
        let weak = self.downgrade();
        imp.entry.connect_sensitive_notify(move |entry| {
            if !entry.is_sensitive()
                && let Some(obj) = weak.upgrade()
            {
                obj.imp().popup.popdown();
            }
        });
        let weak = self.downgrade();
        imp.entry.connect_activate(move |_| {
            if let Some(obj) = weak.upgrade() {
                obj.imp().popup.popdown();
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            weak.upgrade().map_or(glib::Propagation::Proceed, |obj| {
                obj.key_pressed(key, modifiers)
            })
        });
        imp.entry.add_controller(keys);
    }

    pub(crate) fn key_pressed(
        &self,
        key: gdk::Key,
        modifiers: gdk::ModifierType,
    ) -> glib::Propagation {
        let imp = self.imp();
        if !imp.popup.is_visible() {
            if !modifiers.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK)
                || modifiers
                    .intersects(gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK)
                || !matches!(
                    key,
                    gdk::Key::Left
                        | gdk::Key::Right
                        | gdk::Key::Up
                        | gdk::Key::Down
                        | gdk::Key::KP_Left
                        | gdk::Key::KP_Right
                        | gdk::Key::KP_Up
                        | gdk::Key::KP_Down
                )
            {
                return glib::Propagation::Proceed;
            }
            self.open();
        }
        if key == gdk::Key::Escape {
            imp.popup.popdown();
            return glib::Propagation::Stop;
        }
        imp.calendar.entry_key_pressed(&imp.entry, key, modifiers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a GTK display"]
    fn date_input_retains_focus_and_shares_calendar_shortcuts_without_changing_editing_keys() {
        gtk::init().unwrap();
        let input = DateInput::new(
            eventix_locale::default().as_ref(),
            "2024-01-31".parse().unwrap(),
        );
        let other = gtk::Entry::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&input);
        content.append(&other);
        let window = gtk::Window::builder().child(&content).build();
        window.present();
        input.entry().grab_focus();
        let iterate = || while glib::MainContext::default().iteration(false) {};
        iterate();
        let focused = || {
            gtk::prelude::GtkWindowExt::focus(&window)
                .is_some_and(|focus| focus.is_ancestor(&input.entry()))
        };
        assert!(focused());
        assert!(input.imp().popup.is_visible());
        assert_eq!(input.first_child(), Some(input.entry().upcast()));
        assert_eq!(
            input.entry().next_sibling(),
            Some(input.imp().popup.clone().upcast())
        );
        for key in [
            gdk::Key::Left,
            gdk::Key::Right,
            gdk::Key::Up,
            gdk::Key::Down,
            gdk::Key::space,
        ] {
            assert_eq!(
                input.key_pressed(key, gdk::ModifierType::empty()),
                glib::Propagation::Proceed
            );
        }
        assert_eq!(
            input.key_pressed(gdk::Key::Right, gdk::ModifierType::CONTROL_MASK),
            glib::Propagation::Stop
        );
        assert_eq!(input.entry().text(), "2024-02-01");
        assert!(input.imp().popup.is_visible());
        input.key_pressed(gdk::Key::Return, gdk::ModifierType::empty());
        iterate();
        assert_eq!(input.entry().text(), "2024-02-01");
        assert!(!input.imp().popup.is_visible());
        assert!(focused());

        for (key, modifiers, expected) in [
            (
                gdk::Key::Left,
                gdk::ModifierType::CONTROL_MASK,
                "2024-03-30",
            ),
            (
                gdk::Key::Right,
                gdk::ModifierType::CONTROL_MASK,
                "2024-04-01",
            ),
            (gdk::Key::Up, gdk::ModifierType::CONTROL_MASK, "2024-03-24"),
            (
                gdk::Key::Down,
                gdk::ModifierType::CONTROL_MASK,
                "2024-04-07",
            ),
            (gdk::Key::Left, gdk::ModifierType::ALT_MASK, "2024-02-29"),
            (gdk::Key::Right, gdk::ModifierType::ALT_MASK, "2024-04-30"),
            (gdk::Key::Down, gdk::ModifierType::ALT_MASK, "2023-03-31"),
            (gdk::Key::Up, gdk::ModifierType::ALT_MASK, "2025-03-31"),
        ] {
            input.entry().set_text("2024-03-31");
            input.open();
            iterate();
            assert_eq!(input.key_pressed(key, modifiers), glib::Propagation::Stop);
            assert_eq!(
                input.entry().text(),
                if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                    expected
                } else {
                    "2024-03-31"
                }
            );
            input.key_pressed(gdk::Key::Return, gdk::ModifierType::empty());
            iterate();
            assert_eq!(input.entry().text(), expected);
            assert!(focused());
        }

        other.grab_focus();
        input.entry().grab_focus();
        iterate();
        assert!(input.imp().popup.is_visible());
        input.entry().set_text("2026-10-15");
        assert_eq!(
            input.calendar().date(),
            "2026-10-15".parse::<NaiveDate>().unwrap()
        );
        input.key_pressed(gdk::Key::Page_Down, gdk::ModifierType::empty());
        input.key_pressed(gdk::Key::Escape, gdk::ModifierType::empty());
        iterate();
        assert_eq!(input.entry().text(), "2026-10-15");

        other.grab_focus();
        input.entry().grab_focus();
        iterate();
        input.entry().set_text("2026-");
        input.key_pressed(gdk::Key::Return, gdk::ModifierType::empty());
        iterate();
        assert_eq!(input.entry().text(), "2026-");
        assert!(!input.imp().popup.is_visible());
        other.grab_focus();
        input.entry().grab_focus();
        iterate();
        window.child_focus(gtk::DirectionType::TabForward);
        iterate();
        assert!(
            gtk::prelude::GtkWindowExt::focus(&window)
                .is_some_and(|focus| focus.is_ancestor(&other))
        );
        assert!(!input.imp().popup.is_visible());
        input.entry().grab_focus();
        iterate();
        input.entry().set_sensitive(false);
        iterate();
        assert!(!input.imp().popup.is_visible());
        window.close();
    }
}
