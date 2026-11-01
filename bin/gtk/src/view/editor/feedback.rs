// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{accessible, prelude::*};

use super::{
    ItemEditor,
    validation::{Field, Issue},
};

impl ItemEditor {
    pub fn clear_errors(&self) {
        self.error.clear();
        if let Some(input) = self.invalid.take() {
            input.remove_css_class("ev_editor_invalid");
            input.update_state(&[accessible::State::Invalid(
                gtk::AccessibleInvalidState::False,
            )]);
            input.reset_relation(gtk::AccessibleRelation::DescribedBy);
        }
    }

    /// Shows one error at the bottom and reveals its highlighted input.
    pub fn show_validation_error(&self, issue: &Issue) {
        self.clear_errors();
        let message = self
            .error
            .show(&issue.message, self.options.locale.translate("Error"));
        let (input, section) = self.error_target(issue.field);
        input.add_css_class("ev_editor_invalid");
        input.update_state(&[accessible::State::Invalid(
            gtk::AccessibleInvalidState::True,
        )]);
        input.update_relation(&[accessible::Relation::DescribedBy(&[message.upcast_ref()])]);
        self.invalid.replace(Some(input.clone()));

        if let Some(section) = section {
            if section == 1 {
                self.alarm_mode.set_selected(u32::from(matches!(
                    issue.field,
                    Field::PersonalAlarmDate | Field::PersonalAlarmTime
                )));
            }
            self.sections[section].toggle.set_active(true);
        }
        let target = if issue.field == Field::StartDate && !self.has_start.is_active() {
            self.has_start.clone().upcast::<gtk::Widget>()
        } else {
            input.clone()
        };
        if section.is_none() {
            target.grab_focus();
            return;
        }

        // Wait for the section expansion before focusing an input that may still be unmapped.
        let input = input.downgrade();
        let target = target.downgrade();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(310), move || {
            if let Some(input) = input.upgrade()
                && input.is_mapped()
                && input.has_css_class("ev_editor_invalid")
                && let Some(target) = target.upgrade()
            {
                target.grab_focus();
            }
        });
    }

    fn error_target(&self, field: Field) -> (gtk::Widget, Option<usize>) {
        let (input, section): (&gtk::Widget, _) = match field {
            Field::Calendar => (self.calendar.widget.upcast_ref(), None),
            Field::Summary => (self.summary.upcast_ref(), None),
            Field::CompletedDate => (
                self.task_status
                    .as_ref()
                    .map_or(self.summary.upcast_ref(), |status| {
                        status.completed.entry.upcast_ref()
                    }),
                None,
            ),
            Field::StartDate => (self.start.entry.upcast_ref(), Some(0)),
            Field::StartTime => (self.start_time.upcast_ref(), Some(0)),
            Field::EndDate => (self.end.entry.upcast_ref(), Some(0)),
            Field::EndTime => (self.end_time.upcast_ref(), Some(0)),
            Field::Timezone => (self.timezone.upcast_ref(), Some(0)),
            Field::AlarmDate => (self.alarm.date.entry.upcast_ref(), Some(1)),
            Field::AlarmTime => (self.alarm.time.upcast_ref(), Some(1)),
            Field::PersonalAlarmDate => (self.personal_alarm.date.entry.upcast_ref(), Some(1)),
            Field::PersonalAlarmTime => (self.personal_alarm.time.upcast_ref(), Some(1)),
            Field::RecurrenceUntil => (self.recurrence.until.entry.upcast_ref(), Some(3)),
            Field::NewAttendee => (self.new_attendee.upcast_ref(), Some(4)),
        };
        (input.clone(), section)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use eventix_view::editor::{EditorCalendar, EditorOptions};
    use gtk::prelude::*;

    use super::{Field, ItemEditor};

    fn drain() {
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline {
            while gtk::glib::MainContext::default().iteration(false) {}
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn assert_error(editor: &ItemEditor, field: Field) {
        let issue = editor.command().unwrap_err();
        assert_eq!(issue.field, field);
        editor.show_validation_error(&issue);
        assert!(editor.error.widget.reveals_child());
        let (input, _) = editor.error_target(field);
        assert!(input.has_css_class("ev_editor_invalid"));
        assert_eq!(editor.invalid.borrow().as_ref(), Some(&input));
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn editor_reports_one_error_at_a_time_and_reveals_the_affected_field() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        for language in [
            eventix_locale::LocaleType::English,
            eventix_locale::LocaleType::German,
        ] {
            let editor = ItemEditor::new(
                1,
                EditorOptions {
                    kind: eventix_ical::objects::CalCompType::Event,
                    calendars: vec![EditorCalendar {
                        id: "test".into(),
                        name: "Test".into(),
                        background: "#27c281".into(),
                        personal: true,
                    }],
                    last_calendar: None,
                    locations: vec![],
                    contacts: vec![],
                    start: "2026-10-15T10:00:00".parse().unwrap(),
                    locale: eventix_locale::new_with_timezone(
                        &xdg,
                        language,
                        chrono_tz::Europe::Berlin,
                    )
                    .unwrap(),
                },
            );
            let window = gtk::Window::builder()
                .default_width(900)
                .default_height(700)
                .child(&editor.layer)
                .build();
            window.present();
            drain();
            let error_slot = editor.error.widget.parent().unwrap();
            assert!(error_slot.parent().unwrap().has_css_class("ev_editor_actions"));
            let actions = error_slot.next_sibling().unwrap();
            assert!(editor.save.is_ancestor(&actions));
            assert!(editor.cancel.is_ancestor(&actions));
            editor.start.entry.set_text("bad date");
            editor.end.entry.set_text("bad date");
            editor.start_time.set_text("bad time");
            editor.end_time.set_text("bad time");
            editor.alarm.trigger.set_selected(2);
            editor.alarm.date.entry.set_text("bad date");
            editor.alarm.time.set_text("bad time");
            editor.personal_override.set_active(true);
            editor.personal_alarm.trigger.set_selected(2);
            editor.personal_alarm.date.entry.set_text("bad date");
            editor.personal_alarm.time.set_text("bad time");
            editor.recurrence.frequency.set_selected(2);
            editor.recurrence.end.set_selected(2);
            editor.recurrence.until.entry.set_text("bad date");
            editor.new_attendee.set_text("Alice <alice@example.org>");
            assert!(!editor.error.widget.reveals_child());
            assert!(editor.invalid.borrow().is_none());
            assert_error(&editor, Field::Summary);
            assert!(!editor.start.entry.has_css_class("ev_editor_invalid"));
            editor.summary.set_text("Meeting");

            for (field, value) in [
                (Field::StartDate, "2026-10-15"),
                (Field::EndDate, "2026-10-15"),
                (Field::StartTime, "10:00"),
                (Field::EndTime, "11:00"),
                (Field::AlarmDate, "2026-10-15"),
                (Field::AlarmTime, "09:00"),
                (Field::PersonalAlarmDate, "2026-10-15"),
                (Field::PersonalAlarmTime, "09:00"),
                (Field::RecurrenceUntil, "2026-10-16"),
                (Field::NewAttendee, ""),
            ] {
                let previous = editor.invalid.borrow().clone();
                assert_error(&editor, field);
                if let Some(previous) = previous {
                    assert!(!previous.has_css_class("ev_editor_invalid"));
                }
                drain();
                let (input, section) = editor.error_target(field);
                if let Some(section) = section {
                    assert!(editor.sections[section].toggle.is_active());
                }
                let focus = gtk::prelude::GtkWindowExt::focus(&window).unwrap();
                assert!(focus == input || focus.is_ancestor(&input));
                input.downcast::<gtk::Entry>().unwrap().set_text(value);
            }
            assert!(editor.command().is_ok());
            editor.clear_errors();
            assert!(!editor.error.widget.reveals_child());
            assert!(editor.invalid.borrow().is_none());

            editor.end_time.set_text("09:00");
            assert_error(&editor, Field::EndTime);
            editor.end_time.set_text("11:00");
            editor.recurrence.until.entry.set_text("2026-10-14");
            assert_error(&editor, Field::RecurrenceUntil);
            editor.recurrence.end.set_selected(0);
            editor.all_day.set_active(true);
            editor
                .end
                .entry
                .set_text(&chrono::NaiveDate::MAX.to_string());
            assert_error(&editor, Field::EndDate);
            editor.end.entry.set_text("2026-10-15");
            editor.timezone.set_model(None::<&gtk::StringList>);
            assert!(editor.command().is_ok());
            editor.all_day.set_active(false);
            assert_error(&editor, Field::Timezone);
            editor.show_error("Save failed <details>");
            assert!(editor.invalid.borrow().is_none());
            assert!(editor.error.widget.reveals_child());
            window.close();
            drain();
        }
    }
}
