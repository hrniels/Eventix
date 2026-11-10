// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use chrono::Utc;
use eventix_ical::objects::CalAlarm;
use eventix_locale::Locale;
use eventix_state::actions::items::{ItemId, UpdatePersonalAlarms};
use gtk::prelude::*;

use super::size_transition::SizeTransition;
use super::{DetailItem, ReminderHandler, button, label};
use crate::presentation::details::Details;
use crate::widgets::PersonalReminderInput;

pub(super) struct ReminderEditor {
    save: gtk::Button,
    input: Rc<PersonalReminderInput>,
    error: gtk::Label,
    occurrence: bool,
    timezone: chrono_tz::Tz,
    invalid: String,
    description: String,
    initial: Rc<RefCell<Option<Vec<CalAlarm>>>>,
    display: gtk::Box,
    area: SizeTransition,
    summary: gtk::Label,
}

impl ReminderEditor {
    pub(super) fn saved(&self, details: &Details) {
        let initial = if self.occurrence {
            &details.personal_reminders.occurrence
        } else {
            &details.personal_reminders.series
        };
        self.initial.replace(initial.clone());
        self.input.load(initial.as_deref(), &self.timezone);
        self.error.set_visible(false);
        self.summary
            .set_text(details.reminders.as_deref().unwrap_or("−"));
        self.area.show(&self.display);
    }

    pub(super) fn connect_save(&self, item: Option<ItemId>, callback: ReminderHandler) {
        let Some(mut item) = item else { return };
        if !self.occurrence {
            item.recurrence = None;
        }
        let input = self.input.clone();
        let error = self.error.clone();
        let timezone = self.timezone;
        let invalid = self.invalid.clone();
        let description = self.description.clone();
        self.save.connect_clicked(move |_| {
            let alarms = match input.value(&timezone, &invalid, &description) {
                Ok(alarms) => alarms,
                Err(err) => {
                    error.set_text(&err.to_string());
                    error.set_visible(true);
                    return;
                }
            };
            error.set_visible(false);
            callback(UpdatePersonalAlarms {
                item: item.clone(),
                alarms,
            });
        });
    }
}

pub(super) fn content(
    reminders: &str,
    event: &DetailItem<'_>,
    locale: &dyn Locale,
) -> (gtk::Box, Vec<ReminderEditor>) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let display = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let summary = label(reminders);
    display.append(&summary);
    let area = SizeTransition::new();
    area.stack().add_child(&display);
    content.append(&area);
    let mut editors = Vec::new();
    if !event.details.actions.personal_reminders {
        return (content, editors);
    }

    let stack = area.stack();

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    display.append(&buttons);
    let choices = if event.recurrent {
        &[("Edit series", false), ("Edit occ.", true)][..]
    } else {
        &[("Edit", false)][..]
    };
    for &(key, occurrence) in choices {
        let edit = button("pencil", locale.translate(key), "");
        buttons.append(&edit);
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
        stack.add_child(&panel);
        let initial = if occurrence {
            &event.details.personal_reminders.occurrence
        } else {
            &event.details.personal_reminders.series
        };
        let now = Utc::now().with_timezone(locale.timezone());
        let input = Rc::new(PersonalReminderInput::new(
            locale,
            now.date_naive(),
            now.time(),
        ));
        input.load(initial.as_deref(), locale.timezone());
        panel.append(&input.widget);
        let error = label("");
        error.set_visible(false);
        panel.append(&error);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let save = button("", locale.translate("Save"), "");
        let cancel = button("", locale.translate("Cancel"), "");
        actions.append(&save);
        actions.append(&cancel);
        panel.append(&actions);
        let weak_panel = panel.downgrade();
        let weak_area = area.downgrade();
        edit.connect_clicked(move |_| {
            if let (Some(panel), Some(area)) = (weak_panel.upgrade(), weak_area.upgrade()) {
                area.show(&panel);
            }
        });
        let weak_display = display.downgrade();
        let weak_area = area.downgrade();
        let initial = Rc::new(RefCell::new(initial.clone()));
        let reset_initial = initial.clone();
        let reset_input = input.clone();
        let reset_error = error.clone();
        let timezone = *locale.timezone();
        cancel.connect_clicked(move |_| {
            let initial = reset_initial.borrow();
            reset_input.load(initial.as_deref(), &timezone);
            reset_error.set_visible(false);
            if let (Some(display), Some(area)) = (weak_display.upgrade(), weak_area.upgrade()) {
                area.show(&display);
            }
        });
        editors.push(ReminderEditor {
            save,
            input,
            error,
            occurrence,
            timezone: *locale.timezone(),
            invalid: locale.translate("error.valid_date_time").to_owned(),
            description: locale.translate("Reminder").to_owned(),
            initial,
            display: display.clone(),
            area: area.clone(),
            summary: summary.clone(),
        });
    }
    (content, editors)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use crate::presentation::details::{DetailActions, Details, PersonalReminders};
    use chrono::Duration;
    use eventix_ical::objects::{CalAction, CalAlarm, CalRelated, CalTrigger};
    use eventix_state::actions::items::ItemId;
    use gtk::prelude::*;

    use super::{DetailItem, content};

    #[test]
    #[ignore = "requires a GTK display"]
    fn reminder_editor_saves_selected_scope_disables_and_restores_defaults() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let locale = eventix_locale::default();
        let alarm = CalAlarm::new(
            CalAction::Display,
            CalTrigger::Relative {
                duration: Duration::minutes(-15).into(),
                related: CalRelated::Start,
            },
        );
        let mut details = Details {
            date_label: "Date",
            date: String::new(),
            source_date: None,
            location: String::new(),
            description: String::new(),
            status: None,
            recurrence: None,
            reminders: Some("−".into()),
            personal_reminders: PersonalReminders {
                series: Some(vec![alarm.clone()]),
                occurrence: Some(vec![]),
            },
            organizer: None,
            sent_by: None,
            attendees: vec![],
            actions: DetailActions {
                personal_reminders: true,
                read_only: true,
                ..Default::default()
            },
        };
        let (content, editors) = content(
            "−",
            &DetailItem {
                summary: "Meeting",
                calendar: "Test",
                background: "",
                foreground: "",
                details: &details,
                recurrent: true,
                exception: false,
                alarm: false,
            },
            locale.as_ref(),
        );
        let item = ItemId {
            calendar: "test".into(),
            uid: "event".into(),
            recurrence: Some("TU2026-10-13T09:00:00".parse().unwrap()),
        };
        let received = Rc::new(RefCell::new(Vec::new()));
        for editor in &editors {
            let received = received.clone();
            editor.connect_save(
                Some(item.clone()),
                Rc::new(move |command| {
                    received.borrow_mut().push(command);
                }),
            );
            editor.save.emit_clicked();
        }
        assert_eq!(received.borrow().len(), 2);
        assert!(received.borrow()[0].item.recurrence.is_none());
        assert_eq!(
            received.borrow()[0].alarms.as_ref().unwrap()[0].trigger(),
            alarm.trigger()
        );
        assert_eq!(received.borrow()[1].item, item);
        assert!(received.borrow()[1].alarms.as_ref().unwrap().is_empty());

        editors[1].input.overwrite.set_active(false);
        assert!(!editors[1].input.input.widget.is_sensitive());
        editors[1].save.emit_clicked();
        assert!(received.borrow()[2].alarms.is_none());

        let absolute = CalAlarm::new(
            CalAction::Display,
            CalTrigger::Absolute("TU2026-10-13T08:30:00".parse().unwrap()),
        );
        editors[1]
            .input
            .input
            .load(Some(&absolute), locale.timezone());
        editors[1].input.overwrite.set_active(true);
        editors[1].save.emit_clicked();
        assert_eq!(
            received.borrow()[3].alarms.as_ref().unwrap()[0].trigger(),
            absolute.trigger()
        );

        editors[1].input.input.time.set_text("invalid");
        editors[1].save.emit_clicked();
        assert_eq!(received.borrow().len(), 4);
        assert!(editors[1].error.is_visible());
        editors[1].input.overwrite.set_active(false);
        editors[1].save.emit_clicked();
        assert!(received.borrow()[4].alarms.is_none());

        let area = content
            .first_child()
            .unwrap()
            .downcast::<super::SizeTransition>()
            .unwrap();
        let stack = area.stack();
        let display = stack.visible_child().unwrap();
        let buttons = display.last_child().unwrap();
        let edit = buttons
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        let window = gtk::Window::builder()
            .default_width(900)
            .default_height(500)
            .child(&content)
            .build();
        window.present();
        let animate = |expected: &gtk::Widget| {
            let mut heights = Vec::new();
            for _ in 0..30 {
                while gtk::glib::MainContext::default().iteration(false) {}
                assert_eq!(stack.visible_child().as_ref(), Some(expected));
                for page in stack.pages().iter::<gtk::StackPage>().map(Result::unwrap) {
                    assert_eq!(page.child().is_child_visible(), page.child() == *expected);
                }
                heights.push(area.height());
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            heights.dedup();
            heights.len()
        };
        animate(&display);
        let collapsed = area.height();
        edit.emit_clicked();
        let panel = stack.visible_child().unwrap();
        assert!(editors[1].input.widget.is_ancestor(&panel));
        assert_ne!(panel, display);
        assert_eq!(area.height(), collapsed, "content switches before resizing");
        assert!(
            animate(&panel) > 2,
            "the test must sample intermediate animation frames"
        );
        assert_eq!(stack.transition_type(), gtk::StackTransitionType::None);

        details.reminders = Some("Saved reminder".into());
        details.personal_reminders.occurrence = Some(vec![absolute.clone()]);
        for editor in &editors {
            editor.saved(&details);
        }
        assert_eq!(stack.visible_child().as_ref(), Some(&display));
        assert!(animate(&display) > 2);
        assert_eq!(editors[1].summary.text(), "Saved reminder");
        assert!(!editors[1].error.is_visible());

        edit.emit_clicked();
        assert!(animate(&panel) > 2);
        editors[1].input.overwrite.set_active(false);
        let panel = stack.visible_child().unwrap();
        panel
            .last_child()
            .unwrap()
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        assert_eq!(stack.visible_child().as_ref(), Some(&display));
        assert!(animate(&display) > 2);
        assert!(editors[1].input.overwrite.is_active());
        assert_eq!(
            editors[1]
                .input
                .input
                .value_with_text(locale.timezone(), "invalid", "Reminder")
                .unwrap()[0]
                .trigger(),
            absolute.trigger()
        );
        window.close();
    }
}
