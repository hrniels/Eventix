// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveTime, Timelike};
use eventix_locale::Locale;
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

use super::TimePicker;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct TimeInput {
        pub entry: gtk::Entry,
        pub clock: TimePicker,
        pub popup: gtk::Popover,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TimeInput {
        const NAME: &'static str = "EventixTimeInput";
        type Type = super::TimeInput;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for TimeInput {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            self.entry.set_width_chars(5);
            self.entry.set_max_width_chars(5);
            self.entry.set_hexpand(true);
            obj.append(&self.entry);
            self.popup.set_autohide(false);
            self.popup.set_child(Some(&self.clock));
            self.popup.set_parent(&*obj);
            obj.connect_input();
        }

        fn dispose(&self) {
            self.popup.unparent();
        }
    }

    impl WidgetImpl for TimeInput {}
    impl BoxImpl for TimeInput {}
}

glib::wrapper! {
    /// An editable time field with a clock popup that does not take typing focus.
    pub struct TimeInput(ObjectSubclass<imp::TimeInput>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl TimeInput {
    /// Creates a localized time entry and clock with the given initial time.
    pub fn new(locale: &dyn Locale, time: NaiveTime) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().clock.set_locale(locale);
        obj.imp().clock.select_time(time);
        obj.imp().entry.set_text(&time.format("%H:%M").to_string());
        obj.imp()
            .entry
            .set_tooltip_text(Some(locale.translate("Choose time")));
        obj
    }

    /// Returns the editable field used to read and validate the time.
    pub fn entry(&self) -> gtk::Entry {
        self.imp().entry.clone()
    }

    fn open(&self) {
        let imp = self.imp();
        if !imp.entry.is_sensitive() {
            return;
        }
        if let Ok(time) = NaiveTime::parse_from_str(&imp.entry.text(), "%H:%M") {
            imp.clock.select_time(time);
        }
        // Finish any pending popdown before refocusing can reopen the clock.
        imp.popup.set_visible(false);
        imp.popup.popup();
    }

    fn connect_input(&self) {
        let imp = self.imp();
        let weak = self.downgrade();
        imp.entry.connect_changed(move |entry| {
            if let Some(obj) = weak.upgrade()
                && let Ok(time) = NaiveTime::parse_from_str(&entry.text(), "%H:%M")
            {
                obj.imp().clock.select_time(time);
            }
        });
        let weak = self.downgrade();
        imp.clock.connect_time_selected(move |clock| {
            if let Some(obj) = weak.upgrade() {
                obj.imp()
                    .entry
                    .set_text(&clock.time().format("%H:%M").to_string());
                obj.imp().popup.popdown();
                obj.imp().entry.grab_focus();
            }
        });
        let weak = self.downgrade();
        imp.entry.connect_activate(move |_| {
            if let Some(obj) = weak.upgrade() {
                obj.imp().popup.popdown();
            }
        });
        let weak = self.downgrade();
        imp.entry.connect_unmap(move |_| {
            if let Some(obj) = weak.upgrade() {
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
        imp.entry.add_controller(focus);
        let weak = self.downgrade();
        imp.entry.connect_sensitive_notify(move |entry| {
            if !entry.is_sensitive()
                && let Some(obj) = weak.upgrade()
            {
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
        if key == gdk::Key::Escape && imp.popup.is_visible() {
            imp.popup.popdown();
            return glib::Propagation::Stop;
        }
        if !modifiers.contains(gdk::ModifierType::CONTROL_MASK)
            || modifiers.intersects(gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK)
        {
            return glib::Propagation::Proceed;
        }
        let (minutes, delta) = match key {
            gdk::Key::Left => (true, -5),
            gdk::Key::Right => (true, 5),
            gdk::Key::Up => (false, -1),
            gdk::Key::Down => (false, 1),
            _ => return glib::Propagation::Proceed,
        };
        if let Ok(time) = NaiveTime::parse_from_str(&imp.entry.text(), "%H:%M") {
            let time = if minutes {
                time.with_minute((time.minute() as i32 + delta).rem_euclid(60) as u32)
                    .unwrap()
            } else {
                time.with_hour((time.hour() as i32 + delta).rem_euclid(24) as u32)
                    .unwrap()
            };
            imp.entry.set_text(&time.format("%H:%M").to_string());
        }
        imp.clock.show_minutes(minutes);
        glib::Propagation::Stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a GTK display"]
    fn time_input_shortcuts_adjust_components_and_leave_normal_editing_keys_untouched() {
        gtk::init().unwrap();
        let input = TimeInput::new(eventix_locale::default().as_ref(), "23:58".parse().unwrap());
        let entry = input.entry();
        let press = |key| input.key_pressed(key, gdk::ModifierType::CONTROL_MASK);
        let minutes_shown = || {
            input
                .imp()
                .clock
                .first_child()
                .unwrap()
                .last_child()
                .unwrap()
                .has_css_class("suggested-action")
        };
        assert!(!minutes_shown());
        assert_eq!(press(gdk::Key::Right), glib::Propagation::Stop);
        assert!(minutes_shown());
        assert_eq!(entry.text(), "23:03");
        press(gdk::Key::Left);
        assert!(minutes_shown());
        assert_eq!(entry.text(), "23:58");
        press(gdk::Key::Down);
        assert!(!minutes_shown());
        assert_eq!(entry.text(), "00:58");
        press(gdk::Key::Up);
        assert!(!minutes_shown());
        assert_eq!(entry.text(), "23:58");
        assert_eq!(
            input.imp().clock.time(),
            "23:58".parse::<NaiveTime>().unwrap()
        );
        for key in [
            gdk::Key::Left,
            gdk::Key::Right,
            gdk::Key::Up,
            gdk::Key::Down,
        ] {
            assert_eq!(
                input.key_pressed(key, gdk::ModifierType::empty()),
                glib::Propagation::Proceed
            );
        }
        entry.set_text("12:34");
        assert_eq!(
            input.imp().clock.time(),
            "12:34".parse::<NaiveTime>().unwrap()
        );
        entry.set_text("invalid");
        press(gdk::Key::Right);
        assert!(minutes_shown());
        assert_eq!(entry.text(), "invalid");
        assert_eq!(
            input.imp().clock.time(),
            "12:34".parse::<NaiveTime>().unwrap()
        );
    }
}
