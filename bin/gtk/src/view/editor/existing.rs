// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::rc::Rc;

use eventix_ical::objects::{CalAttendee, CalDate, CalDateTime, CalRole, DateContext};
use eventix_state::actions::items::{CreateItem, EditScope, UpdateItem};
use gtk::prelude::*;

use super::{AttendeeInput, AttendeeSection, ItemEditor};
use crate::model::editor::EditorOptions;

impl ItemEditor {
    pub fn edit(id: u64, mut options: EditorOptions, edit: UpdateItem) -> Rc<Self> {
        // Populate before connecting timezone conversion signals.
        options.last_calendar = Some(edit.item.calendar.clone());
        if let Some(start) = edit.fields.start.as_ref().or(edit.fields.end.as_ref()) {
            options.start = DateContext::system()
                .date(start)
                .start_in(options.locale.timezone())
                .naive_local();
        }

        let mut editor = Self::build(id, options);
        editor.populate_existing(&edit);

        let editor = Rc::new(editor);
        editor.initial.replace(editor.command().ok());
        editor.existing.replace(Some(edit));
        editor.connect_timezone();
        editor.update_previews();
        editor.connect_previews();
        editor.connect_task_dates();

        editor
    }

    fn populate_existing(&mut self, edit: &UpdateItem) {
        let title = match (edit.fields.kind, edit.scope) {
            (eventix_ical::objects::CalCompType::Todo, EditScope::Occurrence) => {
                "Edit Task Occurrence"
            }
            (eventix_ical::objects::CalCompType::Todo, EditScope::Following) => {
                "Edit Following Task Occurrences"
            }
            (eventix_ical::objects::CalCompType::Todo, EditScope::Series)
                if edit.fields.recurrence.is_some() =>
            {
                "Edit Task Series"
            }
            (eventix_ical::objects::CalCompType::Todo, EditScope::Series) => "Edit Task",
            (_, EditScope::Occurrence) => "Edit Event Occurrence",
            (_, EditScope::Following) => "Edit Following Event Occurrences",
            (_, EditScope::Series) if edit.fields.recurrence.is_some() => "Edit Event Series",
            (_, EditScope::Series) => "Edit Event",
        };
        self.title = self.options.locale.translate(title).to_string();

        let fields = &edit.fields;
        if let (Some(input), Some(status)) = (&self.task_status, &fields.task_status) {
            input.load(status);
        }

        self.summary.set_text(&fields.summary);
        self.location.set_text(&fields.location);
        self.description.buffer().set_text(&fields.description);
        let selection = self
            .options
            .calendars
            .iter()
            .position(|cal| cal.id == fields.calendar)
            .map_or(gtk::INVALID_LIST_POSITION, |index| index as u32);
        self.calendar.widget.set_selected(selection);
        self.calendar
            .widget
            .set_sensitive(edit.scope != EditScope::Occurrence);

        let timezone = self.load_existing_dates(fields);
        self.load_existing_alarms(fields, &timezone);
        self.recurrence.load(fields.recurrence.as_ref(), &timezone);
        if edit.scope == EditScope::Occurrence {
            self.sections[3].toggle.set_sensitive(false);
            self.recurrence.segments.set_sensitive(false);
        }

        self.load_existing_attendees(&fields.attendees);
    }

    fn load_existing_dates(&self, fields: &CreateItem) -> chrono_tz::Tz {
        self.has_start.set_active(fields.start.is_some());
        self.has_due.set_active(fields.end.is_some());
        let first = fields.start.as_ref().or(fields.end.as_ref());
        let timezone = match first {
            Some(CalDate::DateTime(CalDateTime::Timezone(_, name))) => name.parse().ok(),
            Some(CalDate::DateTime(CalDateTime::Utc(_))) => Some(chrono_tz::UTC),
            _ => None,
        }
        .unwrap_or(*self.options.locale.timezone());
        if let Some(index) = chrono_tz::TZ_VARIANTS
            .iter()
            .position(|zone| *zone == timezone)
        {
            self.timezone.set_selected(index as u32);
        }

        let context = DateContext::system();
        self.all_day
            .set_active(first.is_none_or(|date| matches!(date, CalDate::Date(..))));
        if let Some(start) = &fields.start {
            let start = context.date(start).start_in(&timezone);
            self.start.entry.set_text(&start.date_naive().to_string());
            self.start_time.set_text(&start.format("%H:%M").to_string());
        }

        if let Some(end) = &fields.end {
            let end = context.date(end).start_in(&timezone);
            let end_date = if self.all_day.is_active()
                && fields.kind == eventix_ical::objects::CalCompType::Event
            {
                end.date_naive().pred_opt().unwrap_or(end.date_naive())
            } else {
                end.date_naive()
            };
            self.end.entry.set_text(&end_date.to_string());
            self.end_time.set_text(&end.format("%H:%M").to_string());
        }

        timezone
    }

    fn load_existing_alarms(&self, fields: &CreateItem, timezone: &chrono_tz::Tz) {
        self.alarm.load(fields.alarms.first(), timezone);
        self.personal
            .load(fields.personal_alarms.as_deref(), timezone);
    }

    fn load_existing_attendees(&self, attendees: &[CalAttendee]) {
        let rows = self
            .new_attendee
            .parent()
            .and_then(|row| row.parent())
            .and_then(|widget| widget.first_child())
            .and_downcast::<gtk::Box>()
            .unwrap();

        for attendee in attendees {
            let full_address = attendee.address();
            let address = full_address
                .strip_prefix("mailto:")
                .unwrap_or(&full_address);
            let name = attendee
                .common_name()
                .map_or_else(|| address.to_string(), |name| format!("{name} <{address}>"));
            let mut input = AttendeeInput::new(&name, self.options.locale.as_ref());
            input
                .role
                .set_active(attendee.role() == Some(CalRole::Optional));
            input.original = Some(attendee.clone());
            AttendeeSection::connect_removal_preview(
                &input,
                &self.attendees,
                self.previews[4].downgrade(),
                self.options.locale.translate("None").to_string(),
            );
            rows.append(&input.widget);
            self.attendees.borrow_mut().push(input);
        }
    }

    pub fn content(&self) -> gtk::Widget {
        if self.panel.parent().as_ref() == Some(self.layer.upcast_ref()) {
            self.layer.remove(&self.panel);
        }

        self.panel.clone().upcast()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eventix_ical::{
        col::CalDir,
        objects::{CalCompType, CalDate, CalDateType},
    };
    use eventix_state::{
        CalendarSettings, CollectionSettings, State, SyncerType,
        actions::items::{CreateItem, EditScope, UpdateItem},
    };
    use gtk::prelude::*;

    use crate::model::editor::EditorOptions;
    use crate::view::details::EventDetails;

    #[test]
    #[ignore = "requires a GTK display"]
    fn existing_calendar_overrides_creation_default_and_can_be_changed_for_whole_events() {
        fn displayed_label(widget: &gtk::Widget) -> Option<String> {
            if !widget.is_visible() || !widget.is_child_visible() {
                return None;
            }
            if let Some(label) = widget.downcast_ref::<gtk::Label>() {
                return Some(label.text().to_string());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                if let Some(text) = displayed_label(&widget) {
                    return Some(text);
                }
                child = widget.next_sibling();
            }
            None
        }

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        state.store_mut().add(CalDir::new_empty(
            Arc::new("current".into()),
            tmp.path().into(),
            "Current".into(),
            false,
        ));
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: tmp.path().display().to_string(),
        });
        for (id, name) in [
            ("other", "A default calendar"),
            ("current", "Z current calendar"),
        ] {
            let mut settings = CalendarSettings::default();
            settings.set_enabled(true);
            settings.set_types(vec![CalCompType::Event]);
            settings.set_name(name.into());
            collection.all_calendars_mut().insert(id.into(), settings);
        }
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        let fields = CreateItem {
            kind: CalCompType::Event,
            task_status: None,
            calendar: "current".into(),
            summary: "Event".into(),
            location: String::new(),
            description: String::new(),
            start: Some("TU2026-10-12T09:00:00".parse().unwrap()),
            end: Some("TU2026-10-12T10:00:00".parse().unwrap()),
            recurrence: None,
            alarms: vec![],
            personal_alarms: None,
            attendees: vec![],
        };
        let (mut state, item) = crate::view::editor::tests::execute(Box::new(fields), state);
        state
            .settings_mut()
            .collections_mut()
            .get_mut("local")
            .unwrap()
            .all_calendars_mut()
            .get_mut("current")
            .unwrap()
            .set_types(vec![CalCompType::Todo]);
        let creation = EditorOptions::from_state(&state).unwrap();
        assert_eq!(creation.calendars.len(), 1);
        assert_eq!(creation.calendars[0].id, "other");
        for recurrent in [false, true] {
            let mut edit = UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap();
            if recurrent {
                edit.fields.recurrence = Some("FREQ=DAILY;COUNT=5".parse().unwrap());
            }
            let mut options =
                EditorOptions::for_edit(&state, CalCompType::Event, "current").unwrap();
            options.last_calendar = Some("other".into());
            let editor = super::ItemEditor::edit(1, options, edit.clone());
            let window = gtk::Window::builder().child(&editor.layer).build();
            window.present();
            while gtk::glib::MainContext::default().iteration(false) {}
            assert_eq!(editor.calendar.widget.selected(), 1);
            assert_eq!(
                displayed_label(&editor.calendar.widget.first_child().unwrap()).as_deref(),
                Some("Z current calendar")
            );
            assert_eq!(editor.command().unwrap().calendar, "current");
            assert!(editor.calendar.widget.is_sensitive());
            editor.calendar.widget.set_selected(0);
            assert_eq!(
                displayed_label(&editor.calendar.widget.first_child().unwrap()).as_deref(),
                Some("A default calendar")
            );
            assert_eq!(editor.command().unwrap().calendar, "other");
            window.close();

            for scope in [EditScope::Occurrence, EditScope::Following] {
                edit.scope = scope;
                let editor = super::ItemEditor::edit(
                    2,
                    EditorOptions::for_edit(&state, CalCompType::Event, "current").unwrap(),
                    edit.clone(),
                );
                assert_eq!(editor.command().unwrap().calendar, "current");
                assert_eq!(
                    editor.calendar.widget.is_sensitive(),
                    scope == EditScope::Following
                );
                if scope == EditScope::Following {
                    editor.calendar.widget.set_selected(0);
                    assert_eq!(editor.command().unwrap().calendar, "other");
                }
            }
        }

        let mut edit = UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap();
        let editor = super::ItemEditor::edit(
            3,
            EditorOptions::for_edit(&state, CalCompType::Event, "current").unwrap(),
            edit.clone(),
        );
        editor.summary.set_text("Updated event");
        edit.fields = *editor.command().unwrap();
        let (state, _) = crate::view::editor::tests::execute(Box::new(edit), state);
        assert_eq!(
            crate::model::Event::from_state(&state, &item)
                .unwrap()
                .summary,
            "Updated event"
        );
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn details_open_an_independent_populated_editor_without_restoring_the_popup() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let drain = || {
            for _ in 0..35 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        for all_day in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
            let mut state =
                State::new_with_timezone(xdg.clone(), Some(chrono_tz::Europe::Berlin)).unwrap();
            state.store_mut().add(CalDir::new_empty(
                Arc::new("test".into()),
                tmp.path().into(),
                "Test".into(),
                false,
            ));
            let mut collection = CollectionSettings::new(SyncerType::FileSystem {
                path: tmp.path().display().to_string(),
            });
            let mut settings = CalendarSettings::default();
            settings.set_enabled(true);
            settings.set_types(vec![CalCompType::Event]);
            collection
                .all_calendars_mut()
                .insert("test".into(), settings);
            state
                .settings_mut()
                .collections_mut()
                .insert("local".into(), collection);
            let fields = CreateItem {
                kind: CalCompType::Event,
                task_status: None,
                calendar: "test".into(),
                summary: "Original".into(),
                location: "Office".into(),
                description: "Details\nSecond line".into(),
                start: Some(if all_day {
                    CalDate::new_date("2026-10-12".parse().unwrap(), CalDateType::Inclusive)
                } else {
                    "TTEurope/Berlin;2026-10-12T10:15:30".parse().unwrap()
                }),
                end: Some(if all_day {
                    CalDate::new_date("2026-10-15".parse().unwrap(), CalDateType::Exclusive)
                } else {
                    "TTEurope/Berlin;2026-10-12T11:15:30".parse().unwrap()
                }),
                recurrence: None,
                alarms: vec![],
                personal_alarms: None,
                attendees: vec![],
            };
            let (state, item) = crate::view::editor::tests::execute(Box::new(fields), state);
            let locale = eventix_locale::new_with_timezone(
                &xdg,
                if all_day {
                    eventix_locale::LocaleType::German
                } else {
                    eventix_locale::LocaleType::English
                },
                chrono_tz::Europe::Berlin,
            )
            .unwrap();
            let options = || {
                let mut options = EditorOptions::from_state(&state).unwrap();
                options.locale = locale.clone();
                options
            };
            let parent: crate::view::page::CalendarPage = gtk::glib::Object::new();
            let window = gtk::Window::builder()
                .child(&parent)
                .default_width(1000)
                .default_height(900)
                .build();
            let anchor = gtk::Button::with_label("Event");
            parent.append(&anchor);
            window.present();
            drain();
            let event = crate::model::Event::from_state(&state, &item).unwrap();
            let controller = parent.test_details();
            let popup = EventDetails::new(&event, locale.as_ref());
            controller.show_details(anchor.upcast_ref(), anchor.upcast_ref(), popup.clone());
            drain();
            let editor = parent.test_existing_editor(
                options(),
                UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap(),
            );
            drain();
            let editor_window = editor.summary.root().and_downcast::<gtk::Window>().unwrap();
            assert!(editor_window.is_mapped());
            assert!(!editor_window.is_modal());
            assert_eq!(editor_window.transient_for(), Some(window.clone()));
            assert_eq!(
                editor_window.title().as_deref(),
                Some(locale.translate("Edit Event"))
            );
            assert!(!popup.is_mapped());
            assert!(controller.current().is_none());
            assert!(anchor.is_sensitive());
            assert_eq!(editor.summary.text(), "Original");
            assert_eq!(editor.location.text(), "Office");
            assert_eq!(editor.all_day.is_active(), all_day);
            if all_day {
                assert_eq!(editor.end.entry.text(), "2026-10-14");
            }
            let before = editor.command().unwrap();
            editor.summary.set_text("Discarded");
            parent.dismiss_popovers();
            assert!(parent.editor_open());
            editor.cancel.emit_clicked();
            drain();
            assert!(!parent.editor_open());
            assert!(!editor_window.is_visible());
            assert!(controller.current().is_none());

            let editor = parent.test_existing_editor(
                options(),
                UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap(),
            );
            assert_eq!(editor.summary.text(), "Original");
            editor.summary.set_text("Saved");
            let fields = editor.command().unwrap();
            assert_eq!(fields.start, before.start);
            assert_eq!(fields.end, before.end);
            let mut edit = editor.existing.borrow().clone().unwrap();
            edit.fields = *fields;
            let (state, _) = crate::view::editor::tests::execute(Box::new(edit), state);
            parent.test_saved_editor();
            drain();
            assert!(!parent.editor_open());
            assert!(controller.current().is_none());
            assert_eq!(
                crate::model::Event::from_state(&state, &item)
                    .unwrap()
                    .summary,
                "Saved"
            );
            window.close();
            drain();
        }
    }
}
