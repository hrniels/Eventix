// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{NaiveDate, NaiveTime};
use eventix_ical::objects::CalAlarm;
use eventix_locale::Locale;
use gtk::prelude::*;

use super::AlarmInput;

/// An optional personal reminder that distinguishes inherited defaults from no reminders.
pub(crate) struct PersonalReminderInput {
    pub widget: gtk::Box,
    pub overwrite: gtk::CheckButton,
    pub input: AlarmInput,
}

impl PersonalReminderInput {
    pub fn new(locale: &dyn Locale, date: NaiveDate, time: NaiveTime) -> Self {
        let builder =
            gtk::Builder::from_resource(resource_path!("widgets/personal_reminder_input.ui"));
        let widget: gtk::Box = builder
            .object("personal_reminder_input")
            .expect("personal reminder");
        let overwrite: gtk::CheckButton = builder.object("overwrite").expect("reminder override");
        overwrite.set_label(Some(locale.translate("Overwrite default")));
        let input = AlarmInput::new(locale, date, time);
        input.widget.set_sensitive(false);
        let weak = input.widget.downgrade();
        overwrite.connect_toggled(move |check| {
            if let Some(widget) = weak.upgrade() {
                widget.set_sensitive(check.is_active());
            }
        });
        widget.append(&input.widget);
        Self {
            widget,
            overwrite,
            input,
        }
    }

    pub fn load(&self, alarms: Option<&[CalAlarm]>, timezone: &chrono_tz::Tz) {
        self.input
            .load(alarms.and_then(|alarms| alarms.first()), timezone);
        self.overwrite.set_active(alarms.is_some());
    }

    pub fn value(
        &self,
        timezone: &chrono_tz::Tz,
        invalid: &str,
        description: &str,
    ) -> anyhow::Result<Option<Vec<CalAlarm>>> {
        self.overwrite
            .is_active()
            .then(|| self.input.value_with_text(timezone, invalid, description))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use gtk::prelude::*;

    use super::PersonalReminderInput;

    #[test]
    #[ignore = "requires a GTK display"]
    fn override_distinguishes_inherited_empty_and_explicit_reminders() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let locale = eventix_locale::default();
        let control = PersonalReminderInput::new(
            locale.as_ref(),
            "2026-10-15".parse().unwrap(),
            "10:30:00".parse().unwrap(),
        );
        let value = || {
            control
                .value(&chrono_tz::UTC, "invalid", "Reminder")
                .unwrap()
        };
        assert_eq!(value(), None);
        assert!(!control.input.widget.is_sensitive());

        control.load(Some(&[]), &chrono_tz::UTC);
        assert_eq!(value(), Some(vec![]));
        assert!(control.input.widget.is_sensitive());
        control.input.trigger.set_selected(1);
        let alarms = value().unwrap();
        assert_eq!(alarms.len(), 1);
        control.load(None, &chrono_tz::UTC);
        assert_eq!(value(), None);
        assert!(!control.input.widget.is_sensitive());
        control.load(Some(&alarms), &chrono_tz::UTC);
        assert_eq!(value(), Some(alarms));
    }
}
