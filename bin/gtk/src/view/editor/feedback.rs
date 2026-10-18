// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::rc::Rc;

use gtk::{accessible, prelude::*};

use super::{
    ItemEditor,
    validation::{Field, Issue},
};

pub(super) struct FieldFeedback {
    field: Field,
    input: gtk::Widget,
    message: gtk::Label,
    section: Option<usize>,
    personal: bool,
}

impl FieldFeedback {
    fn new(field: Field, input: &impl IsA<gtk::Widget>, section: Option<usize>) -> Self {
        let input = input.upcast_ref::<gtk::Widget>();

        let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 3);
        wrapper.set_hexpand(input.hexpands());
        wrapper.set_halign(input.halign());
        wrapper.set_valign(gtk::Align::Start);

        // All validated controls are constructed inside box rows.
        let parent = input
            .parent()
            .and_downcast::<gtk::Box>()
            .expect("field row");
        let previous = input.prev_sibling();
        parent.remove(input);
        parent.insert_child_after(&wrapper, previous.as_ref());

        wrapper.append(input);

        let message = gtk::Label::new(None);
        message.set_xalign(0.0);
        message.set_wrap(true);
        message.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        message.set_max_width_chars(32);
        message.add_css_class("ev_editor_field_error");
        message.set_visible(false);
        wrapper.append(&message);

        Self {
            field,
            input: input.clone(),
            message,
            section,
            personal: matches!(field, Field::PersonalAlarmDate | Field::PersonalAlarmTime),
        }
    }

    fn update(&self, issues: &[Issue]) {
        let issue = issues.iter().find(|issue| issue.field == self.field);
        self.message
            .set_text(issue.map_or("", |issue| issue.message.as_str()));
        self.message.set_visible(issue.is_some());

        self.input
            .update_state(&[accessible::State::Invalid(if issue.is_some() {
                gtk::AccessibleInvalidState::True
            } else {
                gtk::AccessibleInvalidState::False
            })]);

        if issue.is_some() {
            self.input.add_css_class("ev_editor_invalid");
            self.input
                .update_relation(&[accessible::Relation::DescribedBy(&[self
                    .message
                    .upcast_ref()])]);
        } else {
            self.input.remove_css_class("ev_editor_invalid");
            self.input
                .reset_relation(gtk::AccessibleRelation::DescribedBy);
        }
    }
}

impl ItemEditor {
    pub fn clear_errors(&self) {
        self.error.set_visible(false);
        self.present_issues(&[]);
    }

    pub(super) fn install_feedback(&mut self) {
        self.feedback = vec![
            FieldFeedback::new(Field::Calendar, &self.calendar, None),
            FieldFeedback::new(Field::Summary, &self.summary, None),
            FieldFeedback::new(Field::StartDate, &self.start.entry, Some(0)),
            FieldFeedback::new(Field::StartTime, &self.start_time, Some(0)),
            FieldFeedback::new(Field::EndDate, &self.end.entry, Some(0)),
            FieldFeedback::new(Field::EndTime, &self.end_time, Some(0)),
            FieldFeedback::new(Field::Timezone, &self.timezone, Some(0)),
            FieldFeedback::new(Field::AlarmDate, &self.alarm.date.entry, Some(1)),
            FieldFeedback::new(Field::AlarmTime, &self.alarm.time, Some(1)),
            FieldFeedback::new(
                Field::PersonalAlarmDate,
                &self.personal_alarm.date.entry,
                Some(1),
            ),
            FieldFeedback::new(Field::PersonalAlarmTime, &self.personal_alarm.time, Some(1)),
            FieldFeedback::new(
                Field::RecurrenceUntil,
                &self.recurrence.until.entry,
                Some(3),
            ),
            FieldFeedback::new(Field::NewAttendee, &self.new_attendee, Some(4)),
        ];
        if let Some(status) = &self.task_status {
            self.feedback.push(FieldFeedback::new(
                Field::CompletedDate,
                &status.completed.entry,
                None,
            ));
        }
    }

    /// Presents all field errors and reveals the first invalid control on Save.
    pub fn show_validation_errors(&self, issues: &[Issue]) {
        self.summary_touched.set(true);
        self.attendee_save_error
            .set(issues.iter().any(|issue| issue.field == Field::NewAttendee));
        self.error.set_visible(false);
        self.present_issues(issues);
        if let Some(first) = issues.first()
            && let Some(feedback) = self.feedback.iter().find(|item| item.field == first.field)
        {
            if let Some(section) = feedback.section {
                if section == 1 {
                    self.alarm_mode.set_selected(u32::from(feedback.personal));
                }
                self.sections[section].toggle.set_active(true);
            } else {
                feedback.input.grab_focus();
                return;
            }
            // The revealer must finish opening before focus can scroll the entry into view.
            let input = feedback.input.downgrade();
            let target = if first.field == Field::StartDate && !self.has_start.is_active() {
                self.has_start.clone().upcast::<gtk::Widget>()
            } else {
                feedback.input.clone()
            }
            .downgrade();
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
    }

    pub(super) fn connect_feedback(self: &Rc<Self>) {
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        focus.connect_leave(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.summary_touched.set(true);
            }
        });
        self.summary.add_controller(focus);

        self.connect_focus_feedback();
    }

    fn connect_focus_feedback(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.form.connect_root_notify(move |form| {
            let Some(window) = form.root().and_downcast::<gtk::Window>() else {
                return;
            };

            let weak = weak.clone();
            window.connect_focus_widget_notify(move |_| {
                let Some(editor) = weak.upgrade() else {
                    return;
                };

                // Date range synchronization must finish before the errors are checked.
                gtk::glib::idle_add_local_once(move || {
                    if editor.form.is_mapped() {
                        editor.update_feedback();
                    }
                });
            });
        });
    }

    pub(super) fn update_feedback(&self) {
        let mut issues = self.validate();
        if !self.summary_touched.get() {
            issues.retain(|issue| issue.field != Field::Summary);
        }
        if self.new_attendee.text().trim().is_empty() {
            self.attendee_save_error.set(false);
        }
        if !self.attendee_save_error.get() {
            issues.retain(|issue| issue.field != Field::NewAttendee);
        }

        self.present_issues(&issues);
    }

    fn present_issues(&self, issues: &[Issue]) {
        for feedback in &self.feedback {
            feedback.update(issues);
        }
        for (index, section) in self.sections.iter().enumerate() {
            let invalid = self.feedback.iter().any(|feedback| {
                feedback.section == Some(index)
                    && issues.iter().any(|issue| issue.field == feedback.field)
            });
            section.set_invalid(invalid);
        }
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

    fn fields(editor: &ItemEditor) -> Vec<Field> {
        editor
            .command()
            .unwrap_err()
            .iter()
            .map(|issue| issue.field)
            .collect()
    }

    fn change_focus(editor: &ItemEditor) {
        editor.summary.grab_focus();
        editor.location.grab_focus();
        while gtk::glib::MainContext::default().iteration(false) {}
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn field_errors_update_on_focus_changes_and_save_reveals_first_error() {
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
            assert!(!editor.summary.has_css_class("ev_editor_invalid"));
            assert_eq!(
                editor
                    .feedback
                    .iter()
                    .filter(|field| field.message.is_visible())
                    .count(),
                0
            );
            editor.summary.set_text("Meeting");
            assert!(!editor.summary.has_css_class("ev_editor_invalid"));
            editor.summary.set_text("");
            assert!(!editor.summary.has_css_class("ev_editor_invalid"));

            let window = gtk::Window::builder()
                .default_width(900)
                .default_height(700)
                .child(&editor.layer)
                .build();
            window.present();
            editor.summary.grab_focus();
            drain();
            assert!(!editor.summary.has_css_class("ev_editor_invalid"));
            editor.location.grab_focus();
            drain();
            assert!(editor.summary.has_css_class("ev_editor_invalid"));
            editor.summary.set_text("Meeting");
            drain();
            assert!(editor.summary.has_css_class("ev_editor_invalid"));
            change_focus(&editor);
            assert!(!editor.summary.has_css_class("ev_editor_invalid"));
            editor.summary.set_text("");
            drain();
            assert!(!editor.summary.has_css_class("ev_editor_invalid"));
            change_focus(&editor);
            assert!(editor.summary.has_css_class("ev_editor_invalid"));

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
            assert!(!editor.new_attendee.has_css_class("ev_editor_invalid"));
            assert!(!editor.start.entry.has_css_class("ev_editor_invalid"));
            change_focus(&editor);
            assert!(editor.start.entry.has_css_class("ev_editor_invalid"));
            assert!(
                editor
                    .feedback
                    .iter()
                    .filter(|field| field.message.is_visible())
                    .count()
                    == 10
            );
            assert!(
                editor
                    .sections
                    .iter()
                    .all(|section| !section.toggle.is_active())
            );
            let issues = editor.command().unwrap_err();
            assert_eq!(
                issues.iter().map(|issue| issue.field).collect::<Vec<_>>(),
                vec![
                    Field::Summary,
                    Field::StartDate,
                    Field::StartTime,
                    Field::EndDate,
                    Field::EndTime,
                    Field::AlarmDate,
                    Field::AlarmTime,
                    Field::PersonalAlarmDate,
                    Field::PersonalAlarmTime,
                    Field::RecurrenceUntil,
                    Field::NewAttendee,
                ]
            );
            editor.show_validation_errors(&issues);
            assert!(editor.new_attendee.has_css_class("ev_editor_invalid"));
            for index in [0, 1, 3, 4] {
                let heading = editor.sections[index].toggle.child().unwrap();
                let indicator = heading.first_child().unwrap().next_sibling().unwrap();
                assert!(indicator.has_css_class("ev_editor_section_error"));
                assert!(indicator.is_visible());
            }
            assert_eq!(
                editor
                    .feedback
                    .iter()
                    .filter(|field| field.message.is_visible())
                    .count(),
                11
            );
            assert!(editor.summary.has_css_class("ev_editor_invalid"));
            assert!(!editor.error.is_visible());
            drain();
            let focus = gtk::prelude::GtkWindowExt::focus(&window).unwrap();
            assert!(focus == editor.summary || focus.is_ancestor(&editor.summary));
            editor.summary.set_text("Meeting");
            assert!(editor.summary.has_css_class("ev_editor_invalid"));
            change_focus(&editor);
            assert!(!editor.summary.has_css_class("ev_editor_invalid"));
            // A different invalid section is opened only by Save, never by live validation.
            assert!(!editor.sections[0].toggle.is_active());
            editor.show_validation_errors(&editor.command().unwrap_err());
            assert!(editor.sections[0].toggle.is_active());
            drain();
            if let Some(directory) = std::env::var_os("EVENTIX_EDITOR_SCREENSHOTS") {
                let panel = editor.layer.first_child().unwrap();
                let snapshot = gtk::Snapshot::new();
                editor.layer.snapshot_child(&panel, &snapshot);
                let node = snapshot.to_node().unwrap();
                window
                    .renderer()
                    .unwrap()
                    .render_texture(&node, None)
                    .save_to_png(
                        std::path::Path::new(&directory)
                            .join(format!("editor-errors-{language:?}.png")),
                    )
                    .unwrap();
            }
            let focus = gtk::prelude::GtkWindowExt::focus(&window).unwrap();
            assert!(focus == editor.start.entry || focus.is_ancestor(&editor.start.entry));
            assert!(editor.layer.width() <= 900);
            editor.start.entry.set_text("2026-10-15");
            editor.end.entry.set_text("2026-10-15");
            editor.all_day.set_active(true);
            change_focus(&editor);
            assert!(!editor.start_time.has_css_class("ev_editor_invalid"));
            assert!(!editor.end_time.has_css_class("ev_editor_invalid"));
            editor.alarm.trigger.set_selected(0);
            change_focus(&editor);
            assert!(!editor.alarm.date.entry.has_css_class("ev_editor_invalid"));
            editor.personal_override.set_active(false);
            change_focus(&editor);
            assert!(
                !editor
                    .personal_alarm
                    .time
                    .has_css_class("ev_editor_invalid")
            );
            editor.recurrence.frequency.set_selected(0);
            change_focus(&editor);
            assert!(
                !editor
                    .recurrence
                    .until
                    .entry
                    .has_css_class("ev_editor_invalid")
            );
            editor.new_attendee.set_text("");
            change_focus(&editor);
            assert!(!editor.new_attendee.has_css_class("ev_editor_invalid"));
            editor.new_attendee.set_text("Bob <bob@example.org>");
            assert!(!editor.new_attendee.has_css_class("ev_editor_invalid"));
            assert_eq!(fields(&editor), vec![Field::NewAttendee]);
            editor.new_attendee.set_text("");
            assert!(editor.command().is_ok());
            assert!(
                editor
                    .feedback
                    .iter()
                    .all(|field| !field.message.is_visible())
            );
            // Parse failures suppress dependent comparisons, while valid ranges get a precise target.
            editor.all_day.set_active(false);
            assert_eq!(fields(&editor), vec![Field::StartTime, Field::EndTime]);
            editor.start_time.set_text("12:00");
            editor.end_time.set_text("11:00");
            assert_eq!(fields(&editor), vec![Field::EndTime]);
            editor.end_time.set_text("13:00");
            editor.recurrence.frequency.set_selected(2);
            editor.recurrence.until.entry.set_text("2026-10-14");
            assert_eq!(fields(&editor), vec![Field::RecurrenceUntil]);
            editor.recurrence.end.set_selected(0);
            change_focus(&editor);
            assert!(
                !editor
                    .recurrence
                    .until
                    .entry
                    .has_css_class("ev_editor_invalid")
            );
            assert!(editor.command().is_ok());
            // Hidden personal reminder errors are revealed in the correct stack page.
            editor.personal_override.set_active(true);
            editor.alarm_mode.set_selected(0);
            editor.show_validation_errors(&editor.command().unwrap_err());
            assert!(editor.sections[1].toggle.is_active());
            assert_eq!(editor.alarm_mode.selected(), 1);
            drain();
            let focus = gtk::prelude::GtkWindowExt::focus(&window).unwrap();
            assert!(
                focus == editor.personal_alarm.date.entry
                    || focus.is_ancestor(&editor.personal_alarm.date.entry)
            );
            editor.personal_override.set_active(false);
            editor.set_saving(true);
            editor.set_saving(false);
            editor.show_error("Save failed");
            assert!(editor.error.is_visible());
            assert!(editor.save.is_sensitive());
            assert_eq!(editor.command().unwrap().summary, "Meeting");
            editor.all_day.set_active(true);
            editor
                .end
                .entry
                .set_text(&chrono::NaiveDate::MAX.to_string());
            assert_eq!(fields(&editor), vec![Field::EndDate]);
            editor.end.entry.set_text("2026-10-15");
            editor.timezone.set_model(None::<&gtk::StringList>);
            assert!(editor.command().is_ok());
            editor.all_day.set_active(false);
            assert_eq!(fields(&editor), vec![Field::Timezone]);
            window.close();
            drain();
        }
    }
}
