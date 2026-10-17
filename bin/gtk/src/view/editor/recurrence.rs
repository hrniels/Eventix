// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{Datelike, NaiveDate, Weekday};
use eventix_ical::objects::{CalDate, CalRRule, CalRRuleFreq, CalRRuleSide, CalWDayDesc, DayDesc};
use eventix_locale::Locale;
use gtk::prelude::*;

use super::layout::{body_label, choices, segments};
use super::{DateInput, dropdown, spin};

const WEEKDAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

struct PatternWidgets {
    widget: gtk::Box,
    day_button: gtk::CheckButton,
    month_suffix: gtk::Label,
    in_month: gtk::Label,
    weekday_month: gtk::DropDown,
}

pub(super) struct RecurrenceInput {
    pub widget: gtk::Box,
    pub frequency: gtk::DropDown,
    pub segments: gtk::Box,
    interval: gtk::SpinButton,
    pub weekdays: Vec<gtk::CheckButton>,
    pub pattern: gtk::DropDown,
    pub day: gtk::SpinButton,
    pub nth: gtk::DropDown,
    pub weekday: gtk::DropDown,
    pub month: gtk::DropDown,
    pub end: gtk::DropDown,
    pub count: gtk::SpinButton,
    pub until: DateInput,
}

impl RecurrenceInput {
    pub fn new(locale: &dyn Locale, date: NaiveDate) -> Self {
        let frequency = dropdown(
            locale,
            &["None", "Hourly", "Daily", "Weekly", "Monthly", "Yearly"],
        );
        let segments = segments(
            locale,
            &frequency,
            &["None", "Hourly", "Daily", "Weekly", "Monthly", "Annually"],
        );
        let month = Self::create_month(locale, date);
        let weekday = dropdown(
            locale,
            &[
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ],
        );
        weekday.set_selected(date.weekday().num_days_from_monday());
        let input = Self {
            widget: gtk::Box::new(gtk::Orientation::Vertical, 12),
            frequency,
            segments,
            interval: spin(1.0, 255.0, 1.0),
            weekdays: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                .iter()
                .map(|name| gtk::CheckButton::with_label(locale.translate(name)))
                .collect(),
            pattern: gtk::DropDown::from_strings(&["", "", ""]),
            day: spin(1.0, 31.0, date.day().into()),
            nth: dropdown(locale, &["First", "Second", "Third", "Last"]),
            weekday,
            month,
            end: dropdown(locale, &["No end date", "Occurrences", "Repeat until"]),
            count: spin(1.0, 100000.0, 1.0),
            until: DateInput::new(locale, date),
        };
        input.create_settings(locale);
        input
    }

    fn create_month(locale: &dyn Locale, date: NaiveDate) -> gtk::DropDown {
        let month = dropdown(
            locale,
            &[
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ],
        );
        month.set_selected(date.month0());
        month
    }

    fn create_interval(&self, locale: &dyn Locale) -> (gtk::Box, gtk::Label) {
        let every = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        every.append(&body_label(locale, "Every"));
        every.append(&self.interval);
        let unit = body_label(locale, "day(s)");
        every.append(&unit);
        (every, unit)
    }

    fn create_weekly(&self, locale: &dyn Locale) -> gtk::Box {
        let weekly = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let on = body_label(locale, "On");
        on.set_text(&format!("{}:", locale.translate("On")));
        weekly.append(&on);
        for button in &self.weekdays {
            weekly.append(button);
        }
        weekly.set_visible(false);
        weekly
    }

    fn create_patterns(&self, locale: &dyn Locale) -> PatternWidgets {
        let pattern_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let pattern_buttons = choices(locale, &self.pattern, &["Same day", "Day", ""]);
        pattern_box.append(&pattern_buttons[0]);
        let (day_row, month_suffix) = self.create_day_pattern(locale, &pattern_buttons[1]);
        pattern_box.append(&day_row);
        let (nth_row, in_month, weekday_month) =
            self.create_weekday_pattern(locale, &pattern_buttons[2]);
        pattern_box.append(&nth_row);
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let on = body_label(locale, "On");
        on.set_text(&format!("{}:", locale.translate("On")));
        on.set_valign(gtk::Align::Start);
        widget.append(&on);
        widget.append(&pattern_box);
        widget.set_visible(false);
        self.connect_pattern(&weekday_month);
        PatternWidgets {
            widget,
            day_button: pattern_buttons[1].clone(),
            month_suffix,
            in_month,
            weekday_month,
        }
    }

    fn create_day_pattern(
        &self,
        locale: &dyn Locale,
        button: &gtk::CheckButton,
    ) -> (gtk::Box, gtk::Label) {
        let day_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        day_row.append(button);
        day_row.append(&self.month);
        day_row.append(&self.day);
        let month_suffix = body_label(locale, "of the month");
        day_row.append(&month_suffix);
        (day_row, month_suffix)
    }

    fn create_weekday_pattern(
        &self,
        locale: &dyn Locale,
        button: &gtk::CheckButton,
    ) -> (gtk::Box, gtk::Label, gtk::DropDown) {
        let nth_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        nth_row.append(button);
        nth_row.append(&self.nth);
        nth_row.append(&self.weekday);
        let in_month = body_label(locale, "in");
        let weekday_month = gtk::DropDown::new(self.month.model(), None::<gtk::Expression>);
        self.month
            .bind_property("selected", &weekday_month, "selected")
            .bidirectional()
            .sync_create()
            .build();
        nth_row.append(&in_month);
        nth_row.append(&weekday_month);
        (nth_row, in_month, weekday_month)
    }

    fn connect_pattern(&self, weekday_month: &gtk::DropDown) {
        let day_widget = self.day.clone();
        let nth_widget = self.nth.clone();
        let weekday_widget = self.weekday.clone();
        let day_month_widget = self.month.clone();
        let weekday_month_widget = weekday_month.clone();
        self.pattern.connect_selected_notify(move |combo| {
            day_widget.set_sensitive(combo.selected() == 1);
            nth_widget.set_sensitive(combo.selected() == 2);
            weekday_widget.set_sensitive(combo.selected() == 2);
            day_month_widget.set_sensitive(combo.selected() == 1);
            weekday_month_widget.set_sensitive(combo.selected() == 2);
        });
        self.day.set_sensitive(false);
        self.nth.set_sensitive(false);
        self.weekday.set_sensitive(false);
        self.month.set_sensitive(false);
        weekday_month.set_sensitive(false);
    }

    fn create_range(&self, locale: &dyn Locale, settings: &gtk::Box) {
        let heading = body_label(locale, "Range of recurrence");
        heading.set_text(&format!("{}:", locale.translate("Range of recurrence")));
        heading.add_css_class("ev_editor_subheading");
        settings.append(&heading);
        let end_buttons = choices(
            locale,
            &self.end,
            &["No end date", "Create", "Repeat until"],
        );
        settings.append(&end_buttons[0]);
        self.count.set_sensitive(false);
        let count_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        count_row.append(&end_buttons[1]);
        count_row.append(&self.count);
        count_row.append(&body_label(locale, "Repetitions"));
        settings.append(&count_row);
        self.until.widget.set_hexpand(false);
        self.until.widget.set_halign(gtk::Align::Start);
        self.until.widget.set_sensitive(false);
        let until_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        until_row.append(&end_buttons[2]);
        until_row.append(&self.until.widget);
        settings.append(&until_row);
        let count_widget = self.count.clone();
        let until_widget = self.until.widget.clone();
        self.end.connect_selected_notify(move |combo| {
            count_widget.set_sensitive(combo.selected() == 1);
            until_widget.set_sensitive(combo.selected() == 2);
        });
    }

    fn create_settings(&self, locale: &dyn Locale) {
        let settings = gtk::Box::new(gtk::Orientation::Vertical, 8);
        settings.set_visible(false);
        self.widget.append(&settings);
        let (every, unit) = self.create_interval(locale);
        let weekly = self.create_weekly(locale);
        let patterns = self.create_patterns(locale);
        settings.append(&weekly);
        settings.append(&patterns.widget);
        settings.append(&every);
        self.create_range(locale, &settings);
        let units: Vec<_> = ["hour(s)", "day(s)", "week(s)", "month(s)", "year(s)"]
            .iter()
            .map(|key| locale.translate(key).to_string())
            .collect();
        let month_widget = self.month.clone();
        let day_label = locale.translate("Day").to_string();
        self.frequency.connect_selected_notify(move |combo| {
            settings.set_visible(combo.selected() != 0);
            weekly.set_visible(combo.selected() == 3);
            patterns.widget.set_visible(combo.selected() >= 4);
            let yearly = combo.selected() == 5;
            month_widget.set_visible(yearly);
            patterns.in_month.set_visible(yearly);
            patterns.weekday_month.set_visible(yearly);
            patterns.month_suffix.set_visible(!yearly);
            patterns
                .day_button
                .set_label((!yearly).then_some(day_label.as_str()));
            if let Some(text) = combo
                .selected()
                .checked_sub(1)
                .and_then(|index| units.get(index as usize))
            {
                unit.set_text(text);
            }
        });
    }

    pub fn value(&self, until: Option<CalDate>) -> Option<CalRRule> {
        let frequency = match self.frequency.selected() {
            1 => CalRRuleFreq::Hourly,
            2 => CalRRuleFreq::Daily,
            3 => CalRRuleFreq::Weekly,
            4 => CalRRuleFreq::Monthly,
            5 => CalRRuleFreq::Yearly,
            _ => return None,
        };
        let mut rule = CalRRule::default();
        rule.set_frequency(frequency);
        rule.set_interval(self.interval.value_as_int() as u8);
        match frequency {
            CalRRuleFreq::Weekly => self.apply_weekdays(&mut rule),
            CalRRuleFreq::Monthly | CalRRuleFreq::Yearly => {
                self.apply_pattern(&mut rule, frequency)
            }
            _ => {}
        }
        match self.end.selected() {
            1 => rule.set_count(self.count.value_as_int() as u64),
            2 => {
                if let Some(until) = until {
                    rule.set_until(until);
                }
            }
            _ => {}
        }
        Some(rule)
    }

    fn apply_weekdays(&self, rule: &mut CalRRule) {
        let selected: Vec<_> = self
            .weekdays
            .iter()
            .zip(WEEKDAYS)
            .filter(|(button, _)| button.is_active())
            .map(|(_, day)| CalWDayDesc::new(day, None))
            .collect();
        if !selected.is_empty() {
            rule.set_by_day(Some(selected));
        }
    }

    fn apply_pattern(&self, rule: &mut CalRRule, frequency: CalRRuleFreq) {
        match self.pattern.selected() {
            1 => rule.set_by_mon_day(Some(vec![DayDesc::new(
                self.day.value_as_int() as u16,
                CalRRuleSide::Start,
            )])),
            2 => {
                let nth = match self.nth.selected() {
                    3 => (1, CalRRuleSide::End),
                    n => (n as u8 + 1, CalRRuleSide::Start),
                };
                rule.set_by_day(Some(vec![CalWDayDesc::new(
                    WEEKDAYS[self.weekday.selected() as usize],
                    Some(nth),
                )]));
            }
            _ => {}
        }
        if frequency == CalRRuleFreq::Yearly && self.pattern.selected() != 0 {
            rule.set_by_month(Some(vec![self.month.selected() as u8 + 1]));
        }
    }
}
