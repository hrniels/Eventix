// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::rc::Rc;

use eventix_ical::objects::{CalCompType, CalDate, CalDateType};
use eventix_locale::Locale;
use eventix_state::actions::items::CreateItem;
use eventix_view::editor::EditorCalendar;
use gtk::prelude::*;

use crate::view::{calendar_selector::CalendarSelector, editor::DateInput};

pub(crate) struct QuickTask {
    pub widget: gtk::Box,
    pub add: gtk::Button,
    pub summary: gtk::Entry,
    pub due: DateInput,
    due_picker: gtk::MenuButton,
    calendar: CalendarSelector,
}

impl QuickTask {
    pub fn new(locale: &dyn Locale) -> Rc<Self> {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        widget.add_css_class("ev_quick_task");
        widget.set_visible(false);

        let calendar = CalendarSelector::new(&[], None, true);
        widget.append(&calendar.widget);

        let summary = gtk::Entry::builder()
            .hexpand(true)
            .width_chars(1)
            .tooltip_text(locale.translate("Summary"))
            .build();
        widget.append(&summary);

        let due = DateInput::new_popup(locale, chrono::Local::now().date_naive());
        due.entry.set_text("");
        due.entry.set_width_chars(10);
        due.entry.set_max_width_chars(10);
        due.entry.set_hexpand(false);
        due.entry
            .set_tooltip_text(Some(locale.translate("Due date")));
        due.entry.set_icon_from_icon_name(
            gtk::EntryIconPosition::Secondary,
            Some("edit-clear-symbolic"),
        );
        due.entry.connect_icon_press(|entry, _| entry.set_text(""));
        let due_picker = due.picker.clone();
        due_picker.set_icon_name("appointment-soon-symbolic");
        due_picker.set_tooltip_text(Some(locale.translate("Due date")));
        let weak = due_picker.downgrade();
        due.entry.connect_changed(move |entry| {
            if let Some(picker) = weak.upgrade() {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(&entry.text(), "%Y-%m-%d") {
                    picker.set_label(&date.format("%m-%d").to_string());
                } else {
                    picker.set_icon_name("appointment-soon-symbolic");
                }
            }
        });
        widget.append(&due_picker);

        let add = gtk::Button::from_icon_name("list-add-symbolic");
        add.set_tooltip_text(Some(locale.translate("New Task")));
        widget.append(&add);

        Rc::new(Self {
            widget,
            add,
            summary,
            due,
            due_picker,
            calendar,
        })
    }

    pub fn update(
        &self,
        calendars: &[EditorCalendar],
        last_calendar: Option<&str>,
        locale: &dyn Locale,
    ) {
        self.calendar.update(calendars, last_calendar);
        self.widget.set_visible(!calendars.is_empty());
        self.summary
            .set_tooltip_text(Some(locale.translate("Summary")));
        self.due
            .entry
            .set_tooltip_text(Some(locale.translate("Due date")));
        self.due_picker
            .set_tooltip_text(Some(locale.translate("Due date")));
        self.add
            .set_tooltip_text(Some(locale.translate("New Task")));
    }

    pub fn command(&self, locale: &dyn Locale) -> anyhow::Result<Box<CreateItem>> {
        if self.summary.text().trim().is_empty() {
            self.summary.grab_focus();
            anyhow::bail!(locale.translate("error.summary_empty").to_owned());
        }
        let end = if self.due.entry.text().trim().is_empty() {
            None
        } else {
            match self.due.date(locale) {
                Ok(date) => Some(CalDate::Date(date, CalDateType::Inclusive)),
                Err(error) => {
                    if self.widget.root().is_some() {
                        self.due_picker.popup();
                    }
                    self.due.entry.grab_focus();
                    return Err(error);
                }
            }
        };
        let calendar = self.calendar.selected_id().ok_or_else(|| {
            anyhow::anyhow!(
                locale
                    .translate("Could not load local calendars")
                    .to_owned()
            )
        })?;

        Ok(Box::new(CreateItem {
            kind: CalCompType::Todo,
            task_status: None,
            calendar: calendar.to_string(),
            summary: self.summary.text().to_string(),
            location: String::new(),
            description: String::new(),
            start: None,
            end,
            recurrence: None,
            alarms: Vec::new(),
            personal_alarms: None,
            attendees: Vec::new(),
        }))
    }

    pub fn finish(&self, success: bool) {
        self.due_picker.popdown();
        self.widget.set_sensitive(true);
        if success {
            self.summary.set_text("");
            self.due.entry.set_text("");
        }
        self.summary.grab_focus();
    }

    pub fn set_saving(&self) {
        self.due_picker.popdown();
        self.widget.set_sensitive(false);
    }
}

#[cfg(test)]
mod tests {
    use eventix_ical::objects::{CalDate, CalDateType};
    use eventix_view::editor::EditorCalendar;
    use gtk::prelude::*;

    use super::QuickTask;

    #[test]
    #[ignore = "requires a GTK display"]
    fn quick_task_validates_dates_preserves_drafts_and_retains_calendar_selection() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let locale = eventix_locale::default();
        let quick = QuickTask::new(locale.as_ref());
        let mut calendars = vec![
            EditorCalendar {
                id: "first".into(),
                name: "First".into(),
                background: "#ffffff".into(),
                personal: false,
            },
            EditorCalendar {
                id: "last".into(),
                name: "Last".into(),
                background: "#ffffff".into(),
                personal: false,
            },
        ];
        quick.update(&calendars, Some("last"), locale.as_ref());
        assert!(quick.widget.is_visible());
        assert!(quick.command(locale.as_ref()).is_err());

        quick.summary.set_text("Buy groceries");
        let command = quick.command(locale.as_ref()).unwrap();
        assert_eq!(command.calendar, "last");
        assert!(command.start.is_none());
        assert!(command.end.is_none());

        quick.due.entry.set_text("2026-02-30");
        assert!(quick.command(locale.as_ref()).is_err());
        quick.due.entry.set_text("2026-10-24");
        let command = quick.command(locale.as_ref()).unwrap();
        assert_eq!(
            command.end,
            Some(CalDate::Date(
                "2026-10-24".parse().unwrap(),
                CalDateType::Inclusive,
            )),
        );

        assert_eq!(quick.due_picker.label().as_deref(), Some("10-24"));
        quick.set_saving();
        quick.finish(false);
        assert!(quick.widget.is_sensitive());
        assert_eq!(quick.summary.text(), "Buy groceries");
        assert_eq!(quick.due.entry.text(), "2026-10-24");
        quick.update(&calendars, Some("first"), locale.as_ref());
        assert_eq!(quick.command(locale.as_ref()).unwrap().calendar, "last");

        // A renamed calendar refreshes the choices without replacing the selected calendar.
        calendars[1].name = "Renamed".into();
        quick.update(&calendars, Some("first"), locale.as_ref());
        assert_eq!(quick.command(locale.as_ref()).unwrap().calendar, "last");
        assert_eq!(
            quick.calendar.widget.tooltip_text().as_deref(),
            Some("Renamed")
        );

        calendars.pop();
        quick.update(&calendars, None, locale.as_ref());
        assert_eq!(quick.command(locale.as_ref()).unwrap().calendar, "first");
        quick.finish(true);
        assert!(quick.summary.text().is_empty());
        assert!(quick.due.entry.text().is_empty());
        assert_eq!(
            quick.due_picker.icon_name().as_deref(),
            Some("appointment-soon-symbolic")
        );

        quick.update(&[], None, locale.as_ref());
        assert!(!quick.widget.is_visible());
    }
}
