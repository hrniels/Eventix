// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use chrono::{Duration, NaiveDate, NaiveTime};
use eventix_ical::objects::{CalAction, CalAlarm, CalDate, CalRelated, CalTrigger, DateContext};
use eventix_locale::Locale;
use gtk::prelude::*;

use super::{DateInput, TimeInput};

/// Edits an absent, relative, or absolute display reminder.
pub(crate) struct AlarmInput {
    pub widget: gtk::Box,
    pub trigger: gtk::DropDown,
    pub duration: gtk::SpinButton,
    pub unit: gtk::DropDown,
    pub relation: gtk::DropDown,
    relations: RefCell<Vec<u32>>,
    pub relative_row: gtk::Widget,
    pub date: DateInput,
    pub time: gtk::Entry,
}

impl AlarmInput {
    /// Creates a calendar default alarm input, hiding unconfigured absolute dates.
    pub fn for_calendar(locale: &dyn Locale, alarm: Option<&CalAlarm>) -> Self {
        let now = chrono::Utc::now().with_timezone(locale.timezone());
        let input = Self::new(locale, now.date_naive(), now.time());
        input.load(alarm, locale.timezone());
        if !matches!(alarm.map(CalAlarm::trigger), Some(CalTrigger::Absolute(_)))
            && let Some(absolute) = input.widget.last_child()
        {
            // Calendar defaults normally apply relative to each item's dates, not a fixed date.
            absolute.set_visible(false);
        }
        input
    }

    pub fn new(locale: &dyn Locale, date: NaiveDate, time: NaiveTime) -> Self {
        let builder = gtk::Builder::from_resource(resource_path!("widgets/alarm_input.ui"));
        let widget = builder.object("alarm_input").expect("alarm input");
        let names: Vec<_> = ["None", "Relative", "Absolute"]
            .iter()
            .map(|name| locale.translate(name))
            .collect();
        let trigger = gtk::DropDown::from_strings(&names);
        for (index, (id, label)) in [
            ("none", "None"),
            ("relative_choice", "Relative"),
            ("absolute_choice", "Absolute"),
        ]
        .into_iter()
        .enumerate()
        {
            let button: gtk::CheckButton = builder.object(id).expect("alarm choice");
            button.set_label(Some(locale.translate(label)));
            let weak = trigger.downgrade();
            button.connect_toggled(move |button| {
                if button.is_active()
                    && let Some(trigger) = weak.upgrade()
                {
                    trigger.set_selected(index as u32);
                }
            });
            let weak = button.downgrade();
            trigger.connect_selected_notify(move |trigger| {
                if let Some(button) = weak.upgrade() {
                    button.set_active(trigger.selected() == index as u32);
                }
            });
        }
        let duration = builder.object("duration").expect("alarm duration");
        let unit: gtk::DropDown = builder.object("unit").expect("alarm unit");
        let names: Vec<_> = ["Minutes", "Hours", "Days"]
            .iter()
            .map(|name| locale.translate(name))
            .collect();
        unit.set_model(Some(&gtk::StringList::new(&names)));
        let relation: gtk::DropDown = builder.object("relation").expect("alarm relation");
        let names: Vec<_> = ["BeforeStart", "AfterStart", "BeforeEnd", "AfterEnd"]
            .iter()
            .map(|name| locale.translate(name))
            .collect();
        relation.set_model(Some(&gtk::StringList::new(&names)));

        let relative: gtk::Box = builder.object("relative").expect("relative alarm");
        let absolute: gtk::Box = builder.object("absolute").expect("absolute alarm");
        let date = DateInput::new(locale, date);
        date.set_hexpand(false);
        let clock = TimeInput::new(locale, time);
        let time = clock.entry();
        absolute.append(&date);
        absolute.append(&clock);
        trigger.connect_selected_notify(move |combo| {
            relative.set_sensitive(combo.selected() == 1);
            absolute.set_sensitive(combo.selected() == 2);
        });

        Self {
            widget,
            trigger,
            duration,
            unit,
            relation,
            relations: RefCell::new(vec![0, 1, 2, 3]),
            relative_row: builder.object("relative_row").expect("relative alarm row"),
            date,
            time,
        }
    }

    pub fn summary(&self, timezone: &chrono_tz::Tz, locale: &dyn Locale) -> String {
        match self.value(timezone, locale) {
            Ok(alarms) => alarms.first().map_or_else(
                || locale.translate("None").to_string(),
                |alarm| alarm.human(locale).to_string(),
            ),
            Err(_) => locale
                .translate(if self.trigger.selected() == 1 {
                    "Relative"
                } else {
                    "Absolute"
                })
                .to_string(),
        }
    }

    pub fn value(
        &self,
        timezone: &chrono_tz::Tz,
        locale: &dyn Locale,
    ) -> anyhow::Result<Vec<CalAlarm>> {
        self.value_with_text(
            timezone,
            locale.translate("error.valid_date_time"),
            locale.translate("Reminder"),
        )
    }

    pub fn value_with_text(
        &self,
        timezone: &chrono_tz::Tz,
        invalid: &str,
        description: &str,
    ) -> anyhow::Result<Vec<CalAlarm>> {
        let trigger = match self.trigger.selected() {
            1 => self.relative_trigger(),
            2 => self.absolute_trigger(timezone, invalid)?,
            _ => return Ok(Vec::new()),
        };
        Ok(self.alarms(trigger, description))
    }

    pub fn alarms(&self, trigger: CalTrigger, description: &str) -> Vec<CalAlarm> {
        let mut alarm = CalAlarm::new(CalAction::Display, trigger);
        alarm.set_description(Some(description.to_owned()));
        vec![alarm]
    }

    pub fn relative_trigger(&self) -> CalTrigger {
        let relation = self
            .relations
            .borrow()
            .get(self.relation.selected() as usize)
            .copied()
            .unwrap_or(0);
        let amount = i64::from(self.duration.value_as_int());
        let amount = if relation.is_multiple_of(2) {
            -amount
        } else {
            amount
        };
        let duration = match self.unit.selected() {
            1 => Duration::hours(amount),
            2 => Duration::days(amount),
            _ => Duration::minutes(amount),
        };
        CalTrigger::Relative {
            duration: duration.into(),
            related: if relation < 2 {
                CalRelated::Start
            } else {
                CalRelated::End
            },
        }
    }

    pub fn set_dates(&self, start: bool, due: bool, locale: &dyn Locale) {
        let current = self
            .relations
            .borrow()
            .get(self.relation.selected() as usize)
            .copied();
        let available: Vec<_> = (0..4)
            .filter(|index| if *index < 2 { start } else { due })
            .collect();
        let labels = ["BeforeStart", "AfterStart", "BeforeEnd", "AfterEnd"];
        let names: Vec<_> = available
            .iter()
            .map(|index| locale.translate(labels[*index as usize]))
            .collect();
        let selected = current
            .and_then(|current| available.iter().position(|index| *index == current))
            .unwrap_or(0);
        *self.relations.borrow_mut() = available;
        self.relation.set_model(Some(&gtk::StringList::new(&names)));
        self.relation.set_selected(selected as u32);
        self.relative_row.set_visible(start || due);
        if !start && !due && self.trigger.selected() == 1 {
            self.trigger.set_selected(0);
        }
    }

    fn absolute_trigger(
        &self,
        timezone: &chrono_tz::Tz,
        invalid: &str,
    ) -> anyhow::Result<CalTrigger> {
        let date = NaiveDate::parse_from_str(&self.date.entry().text(), "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!(invalid.to_owned()))?;
        let time = NaiveTime::parse_from_str(&self.time.text(), "%H:%M")
            .map_err(|_| anyhow::anyhow!(invalid.to_owned()))?;
        let date = CalDate::new_datetime(date, Some(time), timezone.name());
        Ok(CalTrigger::Absolute(
            DateContext::system().date_to_utc(&date, timezone),
        ))
    }

    pub fn load(&self, alarm: Option<&CalAlarm>, timezone: &chrono_tz::Tz) {
        let Some(alarm) = alarm else {
            self.trigger.set_selected(0);
            return;
        };
        match alarm.trigger() {
            CalTrigger::Relative { duration, related } => {
                let seconds = duration.num_seconds();
                let (unit, divisor) = if seconds % 86400 == 0 {
                    (2, 86400)
                } else if seconds % 3600 == 0 {
                    (1, 3600)
                } else {
                    (0, 60)
                };
                self.duration.set_value((seconds.abs() / divisor) as f64);
                self.unit.set_selected(unit);
                self.relation.set_selected(
                    if *related == CalRelated::End { 2 } else { 0 } + u32::from(seconds > 0),
                );
                self.trigger.set_selected(1);
            }
            CalTrigger::Absolute(date) => {
                let date = DateContext::system().date(date).start_in(timezone);
                self.date.entry().set_text(&date.date_naive().to_string());
                self.time.set_text(&date.format("%H:%M").to_string());
                self.trigger.set_selected(2);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use eventix_ical::objects::{CalRelated, CalTrigger};
    use gtk::prelude::*;

    use super::AlarmInput;

    #[test]
    #[ignore = "requires a GTK display"]
    fn alarm_choices_synchronize_modes_and_preserve_trigger_values() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let locale = eventix_locale::default();
        let alarm = AlarmInput::new(
            locale.as_ref(),
            "2026-10-15".parse().unwrap(),
            "10:30:00".parse().unwrap(),
        );
        let none = alarm
            .widget
            .first_child()
            .and_downcast::<gtk::CheckButton>()
            .unwrap();
        let relative = alarm
            .relative_row
            .first_child()
            .and_downcast::<gtk::CheckButton>()
            .unwrap();
        let absolute_row = alarm.widget.last_child().unwrap();
        let absolute = absolute_row
            .first_child()
            .and_downcast::<gtk::CheckButton>()
            .unwrap();
        let relative_controls = alarm.relative_row.last_child().unwrap();
        let absolute_controls = absolute_row.last_child().unwrap();
        assert!(none.is_active());
        assert!(
            alarm
                .value(&chrono_tz::UTC, locale.as_ref())
                .unwrap()
                .is_empty()
        );
        assert!(!relative_controls.is_sensitive() && !absolute_controls.is_sensitive());

        relative.set_active(true);
        assert_eq!(alarm.trigger.selected(), 1);
        assert!(!none.is_active() && relative_controls.is_sensitive());
        assert!(!absolute_controls.is_sensitive());
        assert!(
            matches!(alarm.value(&chrono_tz::UTC, locale.as_ref()).unwrap()[0].trigger(),
            CalTrigger::Relative { duration, related: CalRelated::Start }
                if *duration == chrono::Duration::minutes(-15).into())
        );

        absolute.set_active(true);
        assert_eq!(alarm.trigger.selected(), 2);
        assert!(!relative.is_active() && !relative_controls.is_sensitive());
        assert!(absolute_controls.is_sensitive());
        assert!(
            matches!(alarm.value(&chrono_tz::UTC, locale.as_ref()).unwrap()[0].trigger(),
            CalTrigger::Absolute(date) if date.to_string() == "TU2026-10-15T10:30:00")
        );

        alarm.trigger.set_selected(1);
        assert!(relative.is_active() && !absolute.is_active());
        alarm.set_dates(false, false, locale.as_ref());
        assert!(none.is_active());
        assert!(!alarm.relative_row.is_visible());
        assert!(
            alarm
                .value(&chrono_tz::UTC, locale.as_ref())
                .unwrap()
                .is_empty()
        );
    }
}
