// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::Cell, sync::OnceLock};

use chrono::{Datelike, NaiveDate, Utc};
use eventix_locale::Locale;
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

use super::MONTHS;

mod imp {
    use super::*;

    pub struct MonthPicker {
        pub selected: Cell<NaiveDate>,
        pub cursor: Cell<NaiveDate>,
        pub today: Cell<NaiveDate>,
        pub timezone: Cell<chrono_tz::Tz>,
        pub heading: gtk::Label,
        pub months: [gtk::Button; 12],
        pub navigation: [gtk::Button; 2],
    }

    impl Default for MonthPicker {
        fn default() -> Self {
            let today = Utc::now().date_naive();
            let month = today.with_day(1).unwrap();
            Self {
                selected: Cell::new(month),
                cursor: Cell::new(month),
                today: Cell::new(today),
                timezone: Cell::new(chrono_tz::UTC),
                heading: gtk::Label::new(None),
                months: std::array::from_fn(|_| gtk::Button::new()),
                navigation: std::array::from_fn(|_| gtk::Button::new()),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MonthPicker {
        const NAME: &'static str = "EventixMonthPicker";
        type Type = super::MonthPicker;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for MonthPicker {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![glib::subclass::Signal::builder("month-selected").build()])
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_orientation(gtk::Orientation::Vertical);
            obj.set_spacing(4);
            obj.set_focusable(true);
            obj.add_css_class("ev_date_picker");
            obj.add_css_class("ev_month_picker");

            let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            self.heading.set_hexpand(true);
            self.heading.add_css_class("ev_date_picker_heading");
            for (index, icon) in ["go-previous-symbolic", "go-next-symbolic"]
                .into_iter()
                .enumerate()
            {
                if index == 1 {
                    header.append(&self.heading);
                }
                let button = &self.navigation[index];
                button.set_icon_name(icon);
                button.add_css_class("flat");
                header.append(button);
                let weak = obj.downgrade();
                button.connect_clicked(move |_| {
                    if let Some(obj) = weak.upgrade() {
                        obj.move_month(if index == 0 { -12 } else { 12 });
                    }
                });
            }
            obj.append(&header);

            let grid = gtk::Grid::builder()
                .column_homogeneous(true)
                .row_homogeneous(true)
                .vexpand(true)
                .build();
            for (index, button) in self.months.iter().enumerate() {
                button.set_focusable(false);
                button.add_css_class("flat");
                button.add_css_class("ev_month_picker_month");
                grid.attach(button, (index % 3) as i32, (index / 3) as i32, 1, 1);
                let weak = obj.downgrade();
                button.connect_clicked(move |_| {
                    let Some(obj) = weak.upgrade() else { return };
                    if let Some(date) = obj.imp().cursor.get().with_month(index as u32 + 1) {
                        obj.select_month(date);
                        obj.confirm();
                    }
                });
            }
            obj.append(&grid);
            obj.set_locale(eventix_locale::default().as_ref());

            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            let weak = obj.downgrade();
            keys.connect_key_pressed(move |_, key, _, modifiers| {
                weak.upgrade().map_or(glib::Propagation::Proceed, |obj| {
                    obj.key_pressed(key, modifiers)
                })
            });
            obj.add_controller(keys);

            let weak = obj.downgrade();
            obj.connect_map(move |_| {
                if let Some(obj) = weak.upgrade() {
                    obj.imp().today.set(
                        Utc::now()
                            .with_timezone(&obj.imp().timezone.get())
                            .date_naive(),
                    );
                    obj.select_month(obj.date());
                    obj.grab_focus();
                }
            });
        }
    }

    impl WidgetImpl for MonthPicker {}
    impl BoxImpl for MonthPicker {}
}

glib::wrapper! {
    /// A localized month selector with three columns and year navigation.
    pub struct MonthPicker(ObjectSubclass<imp::MonthPicker>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl MonthPicker {
    /// Updates translated labels and the current month without changing the selection or cursor.
    pub fn set_locale(&self, locale: &dyn Locale) {
        let imp = self.imp();
        imp.timezone.set(*locale.timezone());
        imp.today
            .set(Utc::now().with_timezone(locale.timezone()).date_naive());
        for (button, name) in imp.months.iter().zip(MONTHS) {
            button.set_label(locale.translate(name));
        }
        for (button, name) in imp.navigation.iter().zip(["Previous year", "Next year"]) {
            let text = locale.translate(name);
            button.set_tooltip_text(Some(text));
            button.update_property(&[gtk::accessible::Property::Label(text)]);
        }
        self.refresh();
    }

    /// Selects the date's month and resets browsing without emitting a selection signal.
    pub fn select_month(&self, date: NaiveDate) {
        let month = date.with_day(1).unwrap();
        self.imp().selected.set(month);
        self.set_cursor(month);
    }

    /// Returns the first day of the committed month, unaffected by unconfirmed browsing.
    pub fn date(&self) -> NaiveDate {
        self.imp().selected.get()
    }

    /// Commits the cursor, including when it is already the selected month.
    pub fn confirm(&self) {
        self.imp().selected.set(self.imp().cursor.get());
        self.refresh();
        self.emit_by_name::<()>("month-selected", &[]);
    }

    /// Calls `callback` when the user confirms a month, but not on programmatic updates.
    pub fn connect_month_selected(&self, callback: impl Fn(&Self) + 'static) {
        self.connect_local("month-selected", false, move |values| {
            callback(&values[0].get::<Self>().unwrap());
            None
        });
    }

    fn set_cursor(&self, date: NaiveDate) {
        self.imp().cursor.set(date);
        self.refresh();
    }

    fn move_month(&self, months: i32) {
        if let Some(date) = crate::dates::shift_month(self.imp().cursor.get(), months) {
            self.set_cursor(date);
        }
    }

    /// Handles picker navigation before enclosing window shortcuts can consume it.
    pub(crate) fn key_pressed(
        &self,
        key: gdk::Key,
        modifiers: gdk::ModifierType,
    ) -> glib::Propagation {
        if key == gdk::Key::Escape
            && let Some(popover) = self
                .ancestor(gtk::Popover::static_type())
                .and_downcast::<gtk::Popover>()
        {
            popover.popdown();
            return glib::Propagation::Stop;
        }
        let control = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        if (!self.has_focus() && !control)
            || modifiers.intersects(
                gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK
                    | gdk::ModifierType::META_MASK,
            )
        {
            return glib::Propagation::Proceed;
        }
        let forward = if self.direction() == gtk::TextDirection::Rtl {
            -1
        } else {
            1
        };
        let months = match key {
            gdk::Key::Page_Up | gdk::Key::KP_Page_Up => -12,
            gdk::Key::Page_Down | gdk::Key::KP_Page_Down => 12,
            gdk::Key::Left | gdk::Key::KP_Left => -forward,
            gdk::Key::Right | gdk::Key::KP_Right => forward,
            gdk::Key::Up | gdk::Key::KP_Up => {
                if control {
                    -12
                } else {
                    -3
                }
            }
            gdk::Key::Down | gdk::Key::KP_Down => {
                if control {
                    12
                } else {
                    3
                }
            }
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space | gdk::Key::KP_Space
                if !control =>
            {
                self.confirm();
                return glib::Propagation::Stop;
            }
            _ => return glib::Propagation::Proceed,
        };
        self.move_month(months);
        glib::Propagation::Stop
    }

    fn refresh(&self) {
        let imp = self.imp();
        let cursor = imp.cursor.get();
        imp.heading.set_text(&cursor.year().to_string());
        for (button, months) in imp.navigation.iter().zip([-12, 12]) {
            button.set_sensitive(crate::dates::shift_month(cursor, months).is_some());
        }
        for (index, button) in imp.months.iter().enumerate() {
            let date = cursor.with_month(index as u32 + 1).unwrap();
            for (class, active) in [
                (
                    "ev_date_picker_today",
                    date.year() == imp.today.get().year()
                        && date.month() == imp.today.get().month(),
                ),
                ("ev_date_picker_selected", date == imp.selected.get()),
                ("ev_date_picker_cursor", date == cursor),
            ] {
                if active {
                    button.add_css_class(class);
                } else {
                    button.remove_css_class(class);
                }
            }
            let label = format!("{} {}", button.label().unwrap_or_default(), date.year());
            button.update_property(&[gtk::accessible::Property::Label(&label)]);
            if date == cursor {
                self.update_property(&[gtk::accessible::Property::Label(&label)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;

    #[test]
    #[ignore = "requires a GTK display"]
    fn month_picker_localizes_navigates_and_confirms() {
        gtk::init().unwrap();
        let picker: MonthPicker = glib::Object::new();
        let date = "2026-11-24".parse().unwrap();
        picker.select_month(date);
        let selections = Rc::new(Cell::new(0));
        let selected = selections.clone();
        picker.connect_month_selected(move |_| selected.set(selected.get() + 1));
        let window = gtk::Window::builder().child(&picker).build();
        gtk::prelude::GtkWindowExt::set_focus(&window, Some(&picker));
        window.present();
        gtk::test_widget_wait_for_draw(&picker);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !picker.has_focus() && std::time::Instant::now() < deadline {
            while glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(picker.has_focus());
        let imp = picker.imp();
        assert_eq!(picker.date(), "2026-11-01".parse::<NaiveDate>().unwrap());
        assert_eq!(imp.heading.text(), "2026");
        for (key, expected) in [
            (gdk::Key::Left, "2026-10-01"),
            (gdk::Key::Right, "2026-12-01"),
            (gdk::Key::Up, "2026-08-01"),
            (gdk::Key::Down, "2027-02-01"),
            (gdk::Key::KP_Left, "2026-10-01"),
            (gdk::Key::KP_Right, "2026-12-01"),
            (gdk::Key::KP_Up, "2026-08-01"),
            (gdk::Key::KP_Down, "2027-02-01"),
            (gdk::Key::Page_Up, "2025-11-01"),
            (gdk::Key::Page_Down, "2027-11-01"),
        ] {
            picker.select_month(date);
            assert_eq!(
                picker.key_pressed(key, gdk::ModifierType::empty()),
                glib::Propagation::Stop
            );
            assert_eq!(imp.cursor.get(), expected.parse::<NaiveDate>().unwrap());
            assert_eq!(picker.date(), "2026-11-01".parse::<NaiveDate>().unwrap());
            assert_eq!(selections.get(), 0);
        }
        imp.navigation[1].emit_clicked();
        assert_eq!(imp.heading.text(), "2028");
        imp.navigation[0].emit_clicked();
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let german = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::German,
            chrono_tz::Europe::Berlin,
        )
        .unwrap();
        picker.set_locale(german.as_ref());
        assert_eq!(imp.months[2].label().as_deref(), Some("März"));
        assert_eq!(
            imp.navigation[0].tooltip_text().as_deref(),
            Some("Vorheriges Jahr")
        );
        assert_eq!(imp.cursor.get(), "2027-11-01".parse::<NaiveDate>().unwrap());
        picker.key_pressed(gdk::Key::Return, gdk::ModifierType::empty());
        assert_eq!(picker.date(), imp.cursor.get());
        assert_eq!(selections.get(), 1);
        assert!(imp.months[10].has_css_class("ev_date_picker_selected"));
        imp.months[10].emit_clicked();
        assert_eq!(selections.get(), 2);
        imp.months[0].emit_clicked();
        assert_eq!(picker.date(), "2027-01-01".parse::<NaiveDate>().unwrap());
        assert_eq!(selections.get(), 3);

        picker.set_direction(gtk::TextDirection::Rtl);
        picker.key_pressed(gdk::Key::Left, gdk::ModifierType::empty());
        assert_eq!(imp.cursor.get(), "2027-02-01".parse::<NaiveDate>().unwrap());
        picker.set_direction(gtk::TextDirection::Ltr);
        for (boundary, key, navigation) in [
            (NaiveDate::MIN, gdk::Key::Left, 0),
            (NaiveDate::MAX, gdk::Key::Right, 1),
        ] {
            picker.select_month(boundary);
            picker.key_pressed(key, gdk::ModifierType::empty());
            assert_eq!(imp.cursor.get(), boundary.with_day(1).unwrap());
            assert!(!imp.navigation[navigation].is_sensitive());
        }
        window.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn month_picker_reopening_discards_browsing_and_escape_does_not_commit() {
        gtk::init().unwrap();
        let picker: MonthPicker = glib::Object::new();
        let date = "2026-12-01".parse().unwrap();
        picker.select_month(date);
        let popover = gtk::Popover::builder().child(&picker).build();
        let button = gtk::MenuButton::builder().popover(&popover).build();
        let window = gtk::Window::builder().child(&button).build();
        window.present();
        let iterate = || {
            while glib::MainContext::default().iteration(false) {}
        };
        iterate();
        button.popup();
        iterate();
        picker.key_pressed(gdk::Key::Right, gdk::ModifierType::empty());
        assert_eq!(
            picker.imp().cursor.get(),
            "2027-01-01".parse::<NaiveDate>().unwrap()
        );
        picker.key_pressed(gdk::Key::Escape, gdk::ModifierType::empty());
        iterate();
        assert!(!button.is_active());
        assert_eq!(picker.date(), date);
        button.popup();
        iterate();
        assert_eq!(picker.imp().cursor.get(), date);
        assert!(picker.has_focus());
        window.close();
    }
}
