// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    sync::OnceLock,
};

use chrono::{Datelike, Duration, NaiveDate, Utc, Weekday};
use eventix_locale::Locale;
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

use super::MONTHS;

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Returns the 42 cells of a month, leaving unrepresentable boundary dates empty.
fn month_cells(month: NaiveDate, first: Weekday) -> [Option<NaiveDate>; 42] {
    let offset = i64::from(month.weekday().days_since(first));
    std::array::from_fn(|index| month.checked_add_signed(Duration::days(index as i64 - offset)))
}

mod imp {
    use super::*;

    pub struct DatePicker {
        pub selected: Cell<NaiveDate>,
        pub cursor: Cell<NaiveDate>,
        pub month: Cell<NaiveDate>,
        pub first: Cell<Weekday>,
        pub weekly: Cell<bool>,
        pub hovered: Cell<Option<usize>>,
        pub today: Cell<NaiveDate>,
        pub timezone: Cell<chrono_tz::Tz>,
        pub heading: gtk::Label,
        pub weekdays: [gtk::Label; 7],
        pub weeks: [gtk::Label; 6],
        pub days: [gtk::Button; 42],
        pub navigation: [gtk::Button; 4],
        pub months: RefCell<Vec<String>>,
        pub day_names: RefCell<Vec<String>>,
    }

    impl Default for DatePicker {
        fn default() -> Self {
            let today = Utc::now().date_naive();
            Self {
                selected: Cell::new(today),
                cursor: Cell::new(today),
                month: Cell::new(today.with_day(1).unwrap()),
                first: Cell::new(Weekday::Mon),
                weekly: Cell::new(false),
                hovered: Cell::new(None),
                today: Cell::new(today),
                timezone: Cell::new(chrono_tz::UTC),
                heading: gtk::Label::new(None),
                weekdays: std::array::from_fn(|_| gtk::Label::new(None)),
                weeks: std::array::from_fn(|_| gtk::Label::new(None)),
                days: std::array::from_fn(|_| gtk::Button::new()),
                navigation: std::array::from_fn(|_| gtk::Button::new()),
                months: RefCell::default(),
                day_names: RefCell::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DatePicker {
        const NAME: &'static str = "EventixDatePicker";
        type Type = super::DatePicker;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for DatePicker {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![glib::subclass::Signal::builder("day-selected").build()])
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_orientation(gtk::Orientation::Vertical);
            obj.set_spacing(4);
            obj.set_focusable(true);
            obj.add_css_class("ev_date_picker");

            let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            self.heading.set_hexpand(true);
            self.heading.add_css_class("ev_date_picker_heading");

            for (index, (icon, months)) in [
                ("go-first-symbolic", -12),
                ("go-previous-symbolic", -1),
                ("go-next-symbolic", 1),
                ("go-last-symbolic", 12),
            ]
            .into_iter()
            .enumerate()
            {
                if index == 2 {
                    header.append(&self.heading);
                }
                let button = &self.navigation[index];
                button.set_icon_name(icon);
                button.add_css_class("flat");
                header.append(button);
                let weak = obj.downgrade();
                button.connect_clicked(move |_| {
                    if let Some(obj) = weak.upgrade() {
                        obj.move_month(months);
                    }
                });
            }
            obj.append(&header);

            let grid = gtk::Grid::builder()
                .column_homogeneous(true)
                .row_homogeneous(true)
                .build();
            for (column, label) in self.weekdays.iter().enumerate() {
                label.add_css_class("dim-label");
                label.add_css_class("ev_date_picker_weekday");
                grid.attach(label, column as i32 + 1, 0, 1, 1);
            }
            for (row, label) in self.weeks.iter().enumerate() {
                label.add_css_class("dim-label");
                label.add_css_class("ev_date_picker_week_number");
                grid.attach(label, 0, row as i32 + 1, 1, 1);
            }
            for (index, button) in self.days.iter().enumerate() {
                button.set_focusable(false);
                button.add_css_class("flat");
                button.add_css_class("ev_date_picker_day");
                grid.attach(button, (index % 7 + 1) as i32, (index / 7 + 1) as i32, 1, 1);
                let weak = obj.downgrade();
                button.connect_clicked(move |_| {
                    let Some(obj) = weak.upgrade() else { return };
                    let imp = obj.imp();
                    if let Some(date) = month_cells(imp.month.get(), imp.first.get())[index] {
                        obj.select_day(date);
                        obj.confirm();
                    }
                });

                let motion = gtk::EventControllerMotion::new();
                let weak = obj.downgrade();
                motion.connect_enter(move |_, _, _| {
                    if let Some(obj) = weak.upgrade() {
                        obj.imp().hovered.set(Some(index));
                        obj.refresh_hover();
                    }
                });
                let weak = obj.downgrade();
                motion.connect_leave(move |_| {
                    if let Some(obj) = weak.upgrade() {
                        obj.imp().hovered.set(None);
                        obj.refresh_hover();
                    }
                });
                button.add_controller(motion);
            }
            obj.append(&grid);

            obj.set_locale(eventix_locale::default().as_ref(), Weekday::Mon);

            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);

            let weak = obj.downgrade();
            keys.connect_key_pressed(move |_, key, _, modifiers| {
                weak.upgrade().map_or(glib::Propagation::Proceed, |obj| {
                    obj.key_pressed(key, modifiers)
                })
            });
            obj.add_controller(keys);

            let scroll = gtk::EventControllerScroll::new(
                gtk::EventControllerScrollFlags::VERTICAL
                    | gtk::EventControllerScrollFlags::DISCRETE,
            );
            let weak = obj.downgrade();
            scroll.connect_scroll(move |_, _, dy| {
                let Some(obj) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if dy == 0.0 {
                    return glib::Propagation::Proceed;
                }
                obj.move_month(if dy > 0.0 { 1 } else { -1 });
                glib::Propagation::Stop
            });
            obj.add_controller(scroll);

            let weak = obj.downgrade();
            obj.connect_map(move |_| {
                if let Some(obj) = weak.upgrade() {
                    obj.imp().today.set(
                        Utc::now()
                            .with_timezone(&obj.imp().timezone.get())
                            .date_naive(),
                    );
                    obj.select_day(obj.date());
                    obj.grab_focus();
                }
            });
        }
    }

    impl WidgetImpl for DatePicker {}
    impl BoxImpl for DatePicker {}
}

glib::wrapper! {
    /// A day or week calendar with explicit Eventix locale and week ordering.
    pub struct DatePicker(ObjectSubclass<imp::DatePicker>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl DatePicker {
    /// Enables whole-week highlighting and week-sized arrow-key navigation.
    ///
    /// The committed date and browsing cursor are preserved. Weeks follow the configured
    /// first weekday, and confirming a week returns its cursor date via the usual date signal.
    pub fn set_weekly(&self, weekly: bool) {
        self.imp().weekly.set(weekly);
        self.refresh();
    }

    /// Updates labels and week ordering without changing the committed date or cursor.
    pub fn set_locale(&self, locale: &dyn Locale, first: Weekday) {
        let imp = self.imp();
        imp.first.set(first);
        imp.timezone.set(*locale.timezone());
        imp.today
            .set(Utc::now().with_timezone(locale.timezone()).date_naive());
        imp.months.replace(
            MONTHS
                .iter()
                .map(|name| locale.translate(name).to_owned())
                .collect(),
        );

        let month_width = imp
            .months
            .borrow()
            .iter()
            .map(|name| name.chars().count())
            .max()
            .unwrap_or(0);
        imp.heading.set_width_chars((month_width + 8) as i32);

        imp.day_names.replace(
            [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ]
            .iter()
            .map(|name| locale.translate(name).to_owned())
            .collect(),
        );

        for (index, label) in imp.weekdays.iter().enumerate() {
            let day = (first.num_days_from_monday() as usize + index) % 7;
            label.set_text(locale.translate(WEEKDAYS[day]));
        }

        for (button, name) in imp.navigation.iter().zip([
            "Previous year",
            "Previous month",
            "Next month",
            "Next year",
        ]) {
            let text = locale.translate(name);
            button.set_tooltip_text(Some(text));
            button.update_property(&[gtk::accessible::Property::Label(text)]);
        }

        self.refresh();
    }

    /// Sets the date and resets browsing without emitting a selection signal.
    pub fn select_day(&self, date: NaiveDate) {
        self.imp().selected.set(date);
        self.set_cursor(date);
    }

    /// Returns the committed date, unaffected by unconfirmed browsing.
    pub fn date(&self) -> NaiveDate {
        self.imp().selected.get()
    }

    /// Commits the cursor, including when it is already the selected day.
    pub fn confirm(&self) {
        self.imp().selected.set(self.imp().cursor.get());
        self.refresh();
        self.emit_by_name::<()>("day-selected", &[]);
    }

    /// Calls `callback` when the user confirms a date, but not on programmatic updates.
    pub fn connect_day_selected(&self, callback: impl Fn(&Self) + 'static) {
        self.connect_local("day-selected", false, move |values| {
            callback(&values[0].get::<Self>().unwrap());
            None
        });
    }

    fn set_cursor(&self, date: NaiveDate) {
        self.imp().cursor.set(date);
        self.imp().month.set(date.with_day(1).unwrap());
        self.refresh();
    }

    fn move_month(&self, months: i32) {
        if let Some(date) = eventix_view::shift_month(self.imp().cursor.get(), months) {
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

        if matches!(
            key,
            gdk::Key::Page_Up | gdk::Key::KP_Page_Up | gdk::Key::Page_Down | gdk::Key::KP_Page_Down
        ) {
            let forward = matches!(key, gdk::Key::Page_Down | gdk::Key::KP_Page_Down);
            self.move_month(if forward { 1 } else { -1 } * if control { 12 } else { 1 });
            return glib::Propagation::Stop;
        }

        if control {
            let months = match key {
                gdk::Key::Left | gdk::Key::KP_Left => -1,
                gdk::Key::Right | gdk::Key::KP_Right => 1,
                gdk::Key::Up | gdk::Key::KP_Up => -12,
                gdk::Key::Down | gdk::Key::KP_Down => 12,
                _ => return glib::Propagation::Proceed,
            };
            self.move_month(months);
            return glib::Propagation::Stop;
        }

        if matches!(
            key,
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space | gdk::Key::KP_Space
        ) {
            self.confirm();
            return glib::Propagation::Stop;
        }

        let forward = if self.direction() == gtk::TextDirection::Rtl {
            -1
        } else {
            1
        };
        let horizontal_step = if self.imp().weekly.get() { 7 } else { 1 };
        let days = match key {
            gdk::Key::Left | gdk::Key::KP_Left => -forward * horizontal_step,
            gdk::Key::Right | gdk::Key::KP_Right => forward * horizontal_step,
            gdk::Key::Up | gdk::Key::KP_Up => -7,
            gdk::Key::Down | gdk::Key::KP_Down => 7,
            _ => return glib::Propagation::Proceed,
        };
        if let Some(date) = self
            .imp()
            .cursor
            .get()
            .checked_add_signed(Duration::days(days))
        {
            self.set_cursor(date);
        }
        glib::Propagation::Stop
    }

    fn refresh(&self) {
        let imp = self.imp();
        let month = imp.month.get();
        let cells = month_cells(month, imp.first.get());
        for (row, label) in imp.weeks.iter().enumerate() {
            let thursday = cells[row * 7 + Weekday::Thu.days_since(imp.first.get()) as usize];
            label.set_text(
                &thursday.map_or_else(String::new, |date| date.iso_week().week().to_string()),
            );
        }

        imp.heading.set_text(&format!(
            "{} {}",
            imp.months.borrow()[month.month0() as usize],
            month.year()
        ));

        for (button, months) in imp.navigation.iter().zip([-12, -1, 1, 12]) {
            button.set_sensitive(eventix_view::shift_month(imp.cursor.get(), months).is_some());
        }

        for (button, date) in imp.days.iter().zip(cells) {
            button.set_sensitive(date.is_some());
            button.set_label(&date.map_or_else(String::new, |date| date.day().to_string()));
            for (class, active) in [
                (
                    "ev_date_picker_other_month",
                    date.is_some_and(|date| date.month() != month.month()),
                ),
                ("ev_date_picker_today", date == Some(imp.today.get())),
                (
                    "ev_date_picker_selected",
                    date.is_some_and(|date| self.matches_selection(date, imp.selected.get())),
                ),
                (
                    "ev_date_picker_cursor",
                    date.is_some_and(|date| self.matches_selection(date, imp.cursor.get())),
                ),
            ] {
                if active {
                    button.add_css_class(class);
                } else {
                    button.remove_css_class(class);
                }
            }

            if let Some(date) = date {
                let label = format!(
                    "{}, {} {} {}",
                    imp.day_names.borrow()[date.weekday().num_days_from_monday() as usize],
                    date.day(),
                    imp.months.borrow()[date.month0() as usize],
                    date.year()
                );
                button.update_property(&[gtk::accessible::Property::Label(&label)]);
                if date == imp.cursor.get() {
                    self.update_property(&[gtk::accessible::Property::Label(&label)]);
                }
            }
        }
        self.refresh_hover();
    }

    fn refresh_hover(&self) {
        let imp = self.imp();
        for (index, button) in imp.days.iter().enumerate() {
            let hovered = imp.weekly.get()
                && imp
                    .hovered
                    .get()
                    .is_some_and(|hovered| index / 7 == hovered / 7);
            // GTK owns PRELIGHT and updates it on pointer motion; keep week hover independent.
            if hovered && button.is_sensitive() {
                button.add_css_class("ev_date_picker_hover");
            } else {
                button.remove_css_class("ev_date_picker_hover");
            }
        }
    }

    fn matches_selection(&self, date: NaiveDate, selected: NaiveDate) -> bool {
        if !self.imp().weekly.get() {
            return date == selected;
        }

        // Compare offsets instead of constructing a week start that could precede NaiveDate::MIN.
        let offset = (date - selected).num_days()
            + i64::from(selected.weekday().days_since(self.imp().first.get()));
        (0..7).contains(&offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_grid_covers_every_first_weekday_and_leap_day() {
        for month in ["2026-01-01", "2026-11-01", "2024-02-01"] {
            let month: NaiveDate = month.parse().unwrap();
            let mut first = Weekday::Mon;
            for _ in 0..7 {
                let cells = month_cells(month, first);
                assert_eq!(cells[0].unwrap().weekday(), first);
                assert_eq!(
                    cells
                        .iter()
                        .flatten()
                        .filter(|date| date.month() == month.month())
                        .count(),
                    (eventix_view::shift_month(month, 1).unwrap() - month).num_days() as usize
                );
                for days in cells.windows(2) {
                    assert_eq!(days[1].unwrap() - days[0].unwrap(), Duration::days(1));
                }
                first = first.succ();
            }
        }
    }

    #[test]
    fn month_grid_handles_supported_date_boundaries() {
        for date in [NaiveDate::MIN, NaiveDate::MAX] {
            let cells = month_cells(date.with_day(1).unwrap(), Weekday::Sun);
            assert!(cells.contains(&Some(date)));
        }
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn date_picker_localizes_and_keeps_browsing_separate_from_confirmation() {
        use std::rc::Rc;

        gtk::init().unwrap();
        let picker: DatePicker = glib::Object::new();
        let date = "2024-01-31".parse().unwrap();
        picker.select_day(date);
        let selections = Rc::new(Cell::new(0));
        let selected = selections.clone();
        picker.connect_day_selected(move |_| selected.set(selected.get() + 1));
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
        assert_eq!(imp.heading.text(), "January 2024");
        assert_eq!(imp.weekdays[0].text(), "Mon");
        assert!(imp.weeks.iter().all(|label| label.is_visible()));

        let press = |key, modifiers| picker.key_pressed(key, modifiers);
        for (key, expected) in [
            (gdk::Key::Left, "2024-02-29"),
            (gdk::Key::Right, "2024-04-30"),
            (gdk::Key::Up, "2023-03-31"),
            (gdk::Key::Down, "2025-03-31"),
            (gdk::Key::KP_Left, "2024-02-29"),
            (gdk::Key::KP_Right, "2024-04-30"),
            (gdk::Key::KP_Up, "2023-03-31"),
            (gdk::Key::KP_Down, "2025-03-31"),
        ] {
            picker.set_cursor("2024-03-31".parse().unwrap());
            assert_eq!(
                press(key, gdk::ModifierType::CONTROL_MASK),
                glib::Propagation::Stop
            );
            assert_eq!(imp.cursor.get(), expected.parse::<NaiveDate>().unwrap());
            assert_eq!(picker.date(), date);
            assert_eq!(selections.get(), 0);
        }
        picker.select_day(date);
        assert_eq!(
            press(gdk::Key::Page_Down, gdk::ModifierType::empty()),
            glib::Propagation::Stop
        );
        assert_eq!(imp.cursor.get(), "2024-02-29".parse::<NaiveDate>().unwrap());
        assert_eq!(picker.date(), date);
        assert_eq!(selections.get(), 0);
        press(gdk::Key::Page_Down, gdk::ModifierType::CONTROL_MASK);
        assert_eq!(imp.cursor.get(), "2025-02-28".parse::<NaiveDate>().unwrap());
        press(gdk::Key::Return, gdk::ModifierType::empty());
        assert_eq!(picker.date(), imp.cursor.get());
        assert_eq!(selections.get(), 1);
        press(gdk::Key::Left, gdk::ModifierType::empty());
        let cursor = imp.cursor.get();

        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let german = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::German,
            chrono_tz::Europe::Berlin,
        )
        .unwrap();
        picker.set_locale(german.as_ref(), Weekday::Sun);
        assert_eq!(imp.heading.text(), "Februar 2025");
        assert_eq!(imp.weekdays[0].text(), "So");
        assert_eq!(
            imp.navigation[0].tooltip_text().as_deref(),
            Some("Vorheriges Jahr")
        );
        assert_eq!(picker.date(), "2025-02-28".parse::<NaiveDate>().unwrap());
        assert_eq!(selections.get(), 1);
        assert_eq!(imp.cursor.get(), cursor, "locale changes preserve browsing");
        let cells = month_cells(imp.month.get(), Weekday::Sun);
        let index = cells
            .iter()
            .position(|day| *day == Some(picker.date()))
            .unwrap();
        assert!(imp.days[index].has_css_class("ev_date_picker_selected"));
        imp.days[index].emit_clicked();
        assert_eq!(
            selections.get(),
            2,
            "clicking the selected day confirms again"
        );

        picker.select_day(NaiveDate::MAX);
        assert!(!imp.navigation[2].is_sensitive());
        press(gdk::Key::Right, gdk::ModifierType::empty());
        assert_eq!(imp.cursor.get(), NaiveDate::MAX);
        picker.select_day(NaiveDate::MIN);
        assert!(!imp.navigation[1].is_sensitive());
        press(gdk::Key::Left, gdk::ModifierType::empty());
        assert_eq!(imp.cursor.get(), NaiveDate::MIN);
        assert_eq!(selections.get(), 2);
        window.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn date_picker_weekly_highlights_and_navigates_whole_weeks() {
        use std::rc::Rc;

        gtk::init().unwrap();
        let picker: DatePicker = glib::Object::new();
        let date = "2026-12-30".parse().unwrap();
        picker.select_day(date);
        picker.set_weekly(true);
        let selections = Rc::new(Cell::new(0));
        let selected = selections.clone();
        picker.connect_day_selected(move |_| selected.set(selected.get() + 1));
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
        let highlighted = |class| {
            imp.days
                .iter()
                .zip(month_cells(imp.month.get(), imp.first.get()))
                .filter_map(|(button, date)| button.has_css_class(class).then_some(date).flatten())
                .collect::<Vec<_>>()
        };
        let week: Vec<_> = (0..7)
            .map(|offset| "2026-12-28".parse::<NaiveDate>().unwrap() + Duration::days(offset))
            .collect();
        assert_eq!(highlighted("ev_date_picker_selected"), week);
        assert_eq!(highlighted("ev_date_picker_cursor"), week);

        for (key, days) in [
            (gdk::Key::Left, -7),
            (gdk::Key::Right, 7),
            (gdk::Key::Up, -7),
            (gdk::Key::Down, 7),
            (gdk::Key::KP_Left, -7),
            (gdk::Key::KP_Right, 7),
            (gdk::Key::KP_Up, -7),
            (gdk::Key::KP_Down, 7),
        ] {
            picker.select_day(date);
            assert_eq!(
                picker.key_pressed(key, gdk::ModifierType::empty()),
                glib::Propagation::Stop
            );
            assert_eq!(imp.cursor.get(), date + Duration::days(days));
            assert_eq!(highlighted("ev_date_picker_cursor").len(), 7);
            assert_eq!(picker.date(), date);
            assert_eq!(selections.get(), 0);
        }
        picker.key_pressed(gdk::Key::Return, gdk::ModifierType::empty());
        assert_eq!(picker.date(), date + Duration::days(7));
        assert_eq!(selections.get(), 1);

        picker.select_day(date);
        picker.set_direction(gtk::TextDirection::Rtl);
        picker.key_pressed(gdk::Key::Left, gdk::ModifierType::empty());
        assert_eq!(imp.cursor.get(), date + Duration::days(7));
        picker.set_direction(gtk::TextDirection::Ltr);
        picker.select_day(date);
        picker.set_locale(eventix_locale::default().as_ref(), Weekday::Sun);
        assert_eq!(
            highlighted("ev_date_picker_selected")[0],
            "2026-12-27".parse::<NaiveDate>().unwrap()
        );
        assert_eq!(highlighted("ev_date_picker_selected").len(), 7);

        // Clicking any day of a row confirms that week, including across a year boundary.
        let clicked = "2027-01-02".parse().unwrap();
        let index = month_cells(imp.month.get(), imp.first.get())
            .iter()
            .position(|day| *day == Some(clicked))
            .unwrap();
        imp.days[index].emit_clicked();
        assert_eq!(picker.date(), clicked);
        assert_eq!(highlighted("ev_date_picker_selected").len(), 7);
        assert_eq!(selections.get(), 2);

        for boundary in [NaiveDate::MIN, NaiveDate::MAX] {
            picker.select_day(boundary);
            assert!(picker.matches_selection(boundary, boundary));
            let key = if boundary == NaiveDate::MIN {
                gdk::Key::Left
            } else {
                gdk::Key::Right
            };
            picker.key_pressed(key, gdk::ModifierType::empty());
            assert_eq!(imp.cursor.get(), boundary);
        }

        picker.select_day(date);
        picker.set_weekly(false);
        assert_eq!(highlighted("ev_date_picker_selected"), vec![date]);
        assert_eq!(highlighted("ev_date_picker_cursor"), vec![date]);
        picker.key_pressed(gdk::Key::Right, gdk::ModifierType::empty());
        assert_eq!(imp.cursor.get(), date + Duration::days(1));
        assert_eq!(picker.date(), date);
        assert_eq!(selections.get(), 2);
        window.close();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn date_picker_hover_follows_selection_mode_without_committing() {
        gtk::init().unwrap();
        let picker: DatePicker = glib::Object::new();
        let date = "2026-11-11".parse().unwrap();
        picker.select_day(date);
        picker.set_weekly(true);
        let imp = picker.imp();
        let index = month_cells(imp.month.get(), imp.first.get())
            .iter()
            .position(|day| *day == Some(date))
            .unwrap();
        let motion = |index: usize| {
            let controllers = imp.days[index].observe_controllers();
            (0..controllers.n_items())
                .find_map(|index| {
                    controllers
                        .item(index)
                        .and_downcast::<gtk::EventControllerMotion>()
                })
                .unwrap()
        };
        let hovered = || {
            imp.days
                .iter()
                .enumerate()
                .filter_map(|(index, button)| {
                    button
                        .has_css_class("ev_date_picker_hover")
                        .then_some(index)
                })
                .collect::<Vec<_>>()
        };
        motion(index).emit_by_name::<()>("enter", &[&0.0f64, &0.0f64]);
        let row = index / 7 * 7;
        assert_eq!(hovered(), (row..row + 7).collect::<Vec<_>>());
        assert_eq!(picker.date(), date);
        assert_eq!(imp.cursor.get(), date);

        // Native hover changes on every pointer move must not alter the weekly highlight.
        for day in row..row + 7 {
            imp.days[day].set_state_flags(gtk::StateFlags::PRELIGHT, false);
            motion(index).emit_by_name::<()>("motion", &[&1.0f64, &1.0f64]);
            assert_eq!(hovered(), (row..row + 7).collect::<Vec<_>>());
            imp.days[day].unset_state_flags(gtk::StateFlags::PRELIGHT);
            assert_eq!(hovered(), (row..row + 7).collect::<Vec<_>>());
        }

        imp.days[index].set_state_flags(gtk::StateFlags::PRELIGHT, false);
        picker.set_weekly(false);
        assert!(hovered().is_empty());
        assert!(
            imp.days[index]
                .state_flags()
                .contains(gtk::StateFlags::PRELIGHT)
        );
        imp.days[index].unset_state_flags(gtk::StateFlags::PRELIGHT);
        picker.set_weekly(true);
        assert_eq!(hovered().len(), 7);
        motion(index).emit_by_name::<()>("leave", &[]);
        assert!(hovered().is_empty());

        let next = index + 7;
        motion(next).emit_by_name::<()>("enter", &[&0.0f64, &0.0f64]);
        assert_eq!(hovered(), (row + 7..row + 14).collect::<Vec<_>>());
        motion(next).emit_by_name::<()>("leave", &[]);
        assert!(hovered().is_empty());
        assert_eq!(picker.date(), date);
        assert_eq!(imp.cursor.get(), date);
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn date_picker_scroll_browses_months_without_committing() {
        use std::rc::Rc;

        gtk::init().unwrap();
        let picker: DatePicker = glib::Object::new();
        let date = "2024-01-31".parse().unwrap();
        picker.select_day(date);
        let selections = Rc::new(Cell::new(0));
        let selected = selections.clone();
        picker.connect_day_selected(move |_| selected.set(selected.get() + 1));

        let controllers = picker.observe_controllers();
        let scroll = (0..controllers.n_items())
            .find_map(|index| {
                controllers
                    .item(index)
                    .and_downcast::<gtk::EventControllerScroll>()
            })
            .unwrap();
        assert_eq!(
            scroll.flags(),
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE
        );
        let emit = |dx, dy| scroll.emit_by_name::<bool>("scroll", &[&dx, &dy]);
        assert!(!emit(1.0f64, 0.0f64));
        assert_eq!(picker.imp().cursor.get(), date);

        for weekly in [false, true] {
            picker.set_weekly(weekly);
            for (start, dy, expected) in [
                ("2024-01-31", 1.0f64, "2024-02-29"),
                ("2024-03-31", -1.0, "2024-02-29"),
                ("2024-12-15", 1.0, "2025-01-15"),
                ("2024-01-15", -1.0, "2023-12-15"),
            ] {
                picker.set_cursor(start.parse().unwrap());
                assert!(emit(0.0f64, dy));
                assert_eq!(
                    picker.imp().cursor.get(),
                    expected.parse::<NaiveDate>().unwrap()
                );
                assert_eq!(
                    picker.imp().heading.text(),
                    match expected {
                        "2024-02-29" => "February 2024",
                        "2025-01-15" => "January 2025",
                        _ => "December 2023",
                    }
                );
                assert_eq!(picker.date(), date);
                assert_eq!(selections.get(), 0);
            }
        }

        for (boundary, dy) in [(NaiveDate::MIN, -1.0f64), (NaiveDate::MAX, 1.0)] {
            picker.set_cursor(boundary);
            assert!(emit(0.0f64, dy));
            assert_eq!(picker.imp().cursor.get(), boundary);
            assert_eq!(picker.date(), date);
            assert_eq!(selections.get(), 0);
        }

        picker.confirm();
        assert_eq!(picker.date(), NaiveDate::MAX);
        assert_eq!(selections.get(), 1);
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn date_picker_reopening_discards_browsing_and_escape_does_not_commit() {
        gtk::init().unwrap();
        let picker: DatePicker = glib::Object::new();
        let date = "2026-12-31".parse().unwrap();
        picker.select_day(date);
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
        assert_eq!(picker.date(), date);
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

    #[test]
    #[ignore = "requires a GTK display"]
    fn date_picker_spacing_is_independent_of_toolbar_styles() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gdk::Display::default().unwrap());
        let plain: DatePicker = glib::Object::new();
        let toolbar: DatePicker = glib::Object::new();
        let header = gtk::Box::new(gtk::Orientation::Vertical, 0);
        header.add_css_class("ev_panel_header");
        header.append(&toolbar);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        content.append(&plain);
        content.append(&header);
        let window = gtk::Window::builder().child(&content).build();
        window.present();
        gtk::test_widget_wait_for_draw(&toolbar);

        for (plain, toolbar) in plain
            .imp()
            .days
            .iter()
            .zip(&toolbar.imp().days)
            .chain(plain.imp().navigation.iter().zip(&toolbar.imp().navigation))
        {
            for orientation in [gtk::Orientation::Horizontal, gtk::Orientation::Vertical] {
                assert_eq!(
                    plain.measure(orientation, -1),
                    toolbar.measure(orientation, -1)
                );
            }
        }
        assert!(plain.imp().weeks.iter().all(|label| label.is_visible()));
        assert!(toolbar.imp().weeks.iter().all(|label| label.is_visible()));
        window.close();
    }
}
