// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::rc::Rc;

use eventix_ical::objects::{
    CalAttendee, CalDate, CalDateTime, CalRelated, CalRole, CalTrigger, DateContext,
};
use eventix_state::actions::items::{CreateItem, EditScope, UpdateItem};
use eventix_view::editor::EditorOptions;
use gtk::prelude::*;

use super::{AlarmInput, AttendeeInput, AttendeeSection, ItemEditor};

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
        editor.install_feedback();
        let editor = Rc::new(editor);
        editor.initial.replace(editor.command().ok());
        editor.existing.replace(Some(edit));
        editor.connect_timezone();
        editor.update_previews();
        editor.connect_previews();
        editor.connect_task_dates();
        editor
    }

    fn populate_existing(&self, edit: &UpdateItem) {
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
        self.title.set_text(self.options.locale.translate(title));
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
        self.calendar.set_selected(selection);
        self.calendar
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
        self.personal_override
            .set_active(fields.personal_alarms.is_some());
        self.personal_alarm.load(
            fields
                .personal_alarms
                .as_ref()
                .and_then(|alarms| alarms.first()),
            timezone,
        );
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

    pub fn popup_panel(&self) -> gtk::Widget {
        let panel = self.layer.first_child().unwrap();
        self.layer.remove(&panel);
        panel.set_vexpand(false);
        if let Some(scroll) = panel
            .first_child()
            .and_then(|header| header.next_sibling())
            .and_downcast::<gtk::ScrolledWindow>()
        {
            scroll.set_max_content_height(500);
        }
        panel
    }
}

impl AlarmInput {
    fn load(&self, alarm: Option<&eventix_ical::objects::CalAlarm>, timezone: &chrono_tz::Tz) {
        let Some(alarm) = alarm else { return };
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
                self.date.entry.set_text(&date.date_naive().to_string());
                self.time.set_text(&date.format("%H:%M").to_string());
                self.trigger.set_selected(2);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::Arc,
    };

    use eventix_ical::{
        col::CalDir,
        objects::{CalCompType, CalDate, CalDateType},
    };
    use eventix_state::{
        CalendarSettings, CollectionSettings, State, SyncerType,
        actions::items::{CreateItem, EditScope, UpdateItem},
    };
    use eventix_view::{Event, editor::EditorOptions};
    use gtk::prelude::*;

    use crate::view::details::EventDetails;

    fn press_escape(popup: &EventDetails) -> bool {
        let controllers = popup.observe_controllers();
        let keys = controllers
            .iter::<gtk::glib::Object>()
            .filter_map(Result::ok)
            .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .unwrap();
        keys.emit_by_name::<bool>(
            "key-pressed",
            &[
                &gtk::gdk::Key::Escape,
                &0u32,
                &gtk::gdk::ModifierType::empty(),
            ],
        )
    }

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
        let item = fields.execute(&mut state).unwrap();
        for recurrent in [false, true] {
            let mut edit = UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap();
            if recurrent {
                edit.fields.recurrence = Some("FREQ=DAILY;COUNT=5".parse().unwrap());
            }
            let mut options = EditorOptions::load(&state).unwrap();
            options.last_calendar = Some("other".into());
            let editor = super::ItemEditor::edit(1, options, edit.clone());
            let window = gtk::Window::builder().child(&editor.layer).build();
            window.present();
            while gtk::glib::MainContext::default().iteration(false) {}
            assert_eq!(editor.calendar.selected(), 1);
            assert_eq!(
                displayed_label(&editor.calendar.first_child().unwrap()).as_deref(),
                Some("Z current calendar")
            );
            assert_eq!(editor.command().unwrap().calendar, "current");
            assert!(editor.calendar.is_sensitive());
            editor.calendar.set_selected(0);
            assert_eq!(
                displayed_label(&editor.calendar.first_child().unwrap()).as_deref(),
                Some("A default calendar")
            );
            assert_eq!(editor.command().unwrap().calendar, "other");
            window.close();

            for scope in [EditScope::Occurrence, EditScope::Following] {
                edit.scope = scope;
                let editor =
                    super::ItemEditor::edit(2, EditorOptions::load(&state).unwrap(), edit.clone());
                assert_eq!(editor.command().unwrap().calendar, "current");
                assert_eq!(
                    editor.calendar.is_sensitive(),
                    scope == EditScope::Following
                );
                if scope == EditScope::Following {
                    editor.calendar.set_selected(0);
                    assert_eq!(editor.command().unwrap().calendar, "other");
                }
            }
        }
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn details_expand_into_populated_editor_and_return_after_cancel_and_save() {
        gtk::init().unwrap();
        gtk::Settings::default()
            .unwrap()
            .set_gtk_enable_animations(true);
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let app = gtk::Application::builder()
            .application_id("io.github.hrniels.Eventix.Gtk.EditorTest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let drain = || {
            for _ in 0..35 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        let wait_until = |condition: &dyn Fn() -> bool| {
            for _ in 0..200 {
                while gtk::glib::MainContext::default().iteration(false) {}
                if condition() {
                    break;
                }
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
            let item = fields.execute(&mut state).unwrap();
            let event = Event::load(&state, &item).unwrap();
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
                let mut options = EditorOptions::load(&state).unwrap();
                options.locale = locale.clone();
                options
            };
            let (window, parent) = crate::window::Window::test_new(&app);
            let window = window.upcast::<gtk::Window>();
            window.set_default_size(1000, 900);
            let anchor = gtk::Button::with_label("Event");
            if all_day {
                parent.append(&anchor);
            } else {
                parent.prepend(&anchor);
            }
            window.present();
            drain();
            let controller = parent.test_details();
            let clicked = Rc::new(Cell::new(false));
            let flag = clicked.clone();
            controller.connect_edit(move |_, scope| {
                assert_eq!(scope, EditScope::Series);
                flag.set(true);
            });
            let popup = EventDetails::new(&event, locale.as_ref());
            let original = popup.child().unwrap();
            controller.show_details(anchor.upcast_ref(), anchor.upcast_ref(), popup.clone());
            drain();
            if !all_day {
                let surface = popup.surface();
                let other_window = gtk::Window::builder()
                    .default_width(200)
                    .default_height(100)
                    .build();
                other_window.present();
                wait_until(&|| !window.is_active());
                drain();
                assert!(
                    !popup.is_mapped(),
                    "inactive details must not float above other apps"
                );
                assert_eq!(controller.current().as_ref(), Some(&popup));
                other_window.close();
                window.present();
                wait_until(&|| popup.is_mapped());
                assert!(popup.is_mapped());
                assert_eq!(
                    popup.surface(),
                    surface,
                    "details keep the same native surface"
                );
                drain();
            }
            let stack = popup.child().and_downcast::<gtk::Stack>().unwrap();
            let details_height = popup.height();
            let heights = Rc::new(RefCell::new(Vec::new()));
            let samples = heights.clone();
            popup.add_tick_callback(move |popup, _| {
                samples.borrow_mut().push(popup.height());
                gtk::glib::ControlFlow::Continue
            });
            let width = popup.width();
            let surface = popup.surface().unwrap();
            let native = surface.downcast_ref::<gtk::gdk::Popup>().unwrap();
            let original_x = native.position_x();
            let original_y = native.position_y();
            let original_bottom = original_y + surface.height();
            let closed = Rc::new(Cell::new(0));
            let counter = closed.clone();
            popup.connect_closed(move |_| counter.set(counter.get() + 1));
            fn click_edit(widget: &gtk::Widget, text: &str) {
                if let Some(button) = widget.downcast_ref::<gtk::Button>()
                    && button
                        .child()
                        .and_then(|child| child.last_child())
                        .and_downcast::<gtk::Label>()
                        .is_some_and(|label| label.text() == text)
                {
                    button.emit_clicked();
                    return;
                }
                let mut child = widget.first_child();
                while let Some(widget) = child {
                    child = widget.next_sibling();
                    click_edit(&widget, text);
                }
            }
            click_edit(original.upcast_ref(), locale.translate("Edit"));
            assert!(clicked.get());
            let editor = parent.test_existing_editor(
                options(),
                UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap(),
            );
            assert!(stack.is_transition_running());
            assert!(parent.editor_open());
            assert!(anchor.is_sensitive());
            assert!(!popup.is_autohide());
            assert_eq!(editor.summary.text(), "Original");
            assert_eq!(editor.location.text(), "Office");
            assert_eq!(editor.all_day.is_active(), all_day);
            let before = editor.command().unwrap();
            if all_day {
                assert_eq!(editor.end.entry.text(), "2026-10-14");
            }
            assert_eq!(popup.parent().as_ref(), Some(parent.upcast_ref()));
            editor.focus();
            drain();
            assert!(!stack.is_transition_running());
            let editor_height = popup.height();
            assert!(
                heights
                    .borrow()
                    .iter()
                    .any(|height| *height > details_height.min(editor_height)
                        && *height < details_height.max(editor_height)),
                "entering the editor must interpolate the popup height: {details_height} -> \
                 {editor_height}, samples: {:?}",
                heights.borrow()
            );
            assert!(popup.is_mapped());
            assert_eq!(
                popup.surface(),
                Some(surface.clone()),
                "editing must retain the native popup"
            );
            assert_eq!(closed.get(), 0, "editing must not close and reopen details");
            assert_eq!(popup.width(), width, "details and editing share one width");
            assert!(
                editor.summary.is_sensitive(),
                "the page must not disable the editor"
            );
            assert!(editor.location.is_sensitive());
            assert!(editor.save.is_sensitive());
            assert!(editor.cancel.is_sensitive());
            assert!(editor.summary.grab_focus());
            assert!(
                popup.has_arrow(),
                "editing stays anchored when either side has room"
            );
            assert_eq!(
                popup.position(),
                if all_day {
                    gtk::PositionType::Top
                } else {
                    gtk::PositionType::Bottom
                }
            );
            assert_eq!(
                native.position_x(),
                original_x,
                "editing must not shift horizontally"
            );
            if all_day {
                assert_eq!(
                    native.position_y() + surface.height(),
                    original_bottom,
                    "upward growth keeps the bottom edge anchored"
                );
            } else {
                assert_eq!(
                    native.position_y(),
                    original_y,
                    "downward growth keeps the top edge anchored"
                );
            }
            for (index, section) in editor.sections.iter().enumerate() {
                section.toggle.set_active(true);
                drain();
                assert_eq!(
                    popup.width(),
                    width,
                    "expanded section {index} must not widen the popup (visible={}, mapped={}, \
                     active={}, editor_open={})",
                    popup.is_visible(),
                    popup.is_mapped(),
                    window.is_active(),
                    parent.editor_open()
                );
                if index == 0 {
                    let bounds = |widget: &gtk::Widget| widget.compute_bounds(&popup).unwrap();
                    let grid = editor
                        .start
                        .widget
                        .parent()
                        .unwrap()
                        .parent()
                        .and_downcast::<gtk::Grid>()
                        .unwrap();
                    let start_label = grid.child_at(0, 0).unwrap();
                    let end_label = grid.child_at(2, 0).unwrap();
                    let all_day_label = grid.child_at(0, 1).unwrap();
                    let timezone_label = grid.child_at(2, 1).unwrap();
                    let start = bounds(editor.start.entry.upcast_ref());
                    let end = bounds(editor.end.entry.upcast_ref());
                    for widget in [
                        &start_label,
                        editor.start_time.upcast_ref(),
                        &end_label,
                        editor.end_time.upcast_ref(),
                    ] {
                        let field = bounds(widget);
                        assert!(
                            (field.y() + field.height() / 2.0 - start.y() - start.height() / 2.0)
                                .abs()
                                <= 1.0,
                            "start/end labels, dates, and times must share one row"
                        );
                    }
                    assert!(start.x() < end.x());
                    let all_day = bounds(editor.all_day.upcast_ref());
                    let timezone = bounds(editor.timezone.upcast_ref());
                    assert!(all_day.y() > start.y() + start.height());
                    assert!(
                        (all_day.y() + all_day.height() / 2.0
                            - timezone.y()
                            - timezone.height() / 2.0)
                            .abs()
                            <= 1.0,
                        "all-day and timezone must share the second row"
                    );
                    assert!(all_day.x() + all_day.width() < timezone.x());
                    assert_eq!(
                        bounds(&start_label).x(),
                        bounds(&all_day_label).x(),
                        "Start and All day labels share a grid column"
                    );
                    assert_eq!(
                        bounds(&end_label).x(),
                        bounds(&timezone_label).x(),
                        "End and Timezone labels share a grid column"
                    );
                    assert_eq!(
                        start.x(),
                        all_day.x(),
                        "left-side controls share a grid column"
                    );
                    assert_eq!(
                        end.x(),
                        timezone.x(),
                        "right-side controls share a grid column"
                    );
                }
            }
            assert!(!crate::view::editor::has_nested_popup(&window));
            editor.summary.set_text("Discarded");
            heights.borrow_mut().clear();
            let expanded_height = popup.height();
            if all_day {
                assert!(
                    press_escape(&popup),
                    "Escape must be consumed before GTK closes the native popup"
                );
            } else {
                editor.cancel.emit_clicked();
            }
            assert!(
                !parent.editor_open(),
                "one Escape or Cancel ends editing immediately"
            );
            assert!(
                anchor.is_sensitive(),
                "the calendar stays enabled while editing"
            );
            assert!(stack.is_transition_running());
            drain();
            assert_eq!(stack.visible_child().as_ref(), Some(&original));
            assert_eq!(
                stack.pages().n_items(),
                1,
                "old editor widgets are released after fading out"
            );
            assert!(
                heights
                    .borrow()
                    .iter()
                    .any(|height| *height > details_height.min(expanded_height)
                        && *height < details_height.max(expanded_height)),
                "returning to details must interpolate the popup height"
            );
            assert_eq!(closed.get(), 0, "cancelling must leave details open");
            assert!(popup.is_mapped());
            assert_eq!(popup.width(), width);
            let nested_editor = parent.test_existing_editor(
                options(),
                UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap(),
            );
            nested_editor.sections[0].toggle.set_active(true);
            drain();
            let picker = nested_editor
                .start
                .widget
                .last_child()
                .and_downcast::<gtk::MenuButton>()
                .unwrap();
            picker.popup();
            drain();
            assert!(picker.popover().unwrap().is_visible());
            crate::window::dismiss_popups_on_surface(
                &window,
                &parent,
                &picker.popover().unwrap().surface().unwrap(),
            );
            assert!(
                parent.editor_open(),
                "nested popup clicks must not discard the editor"
            );
            assert!(
                !press_escape(&popup),
                "a nested date picker must get first refusal"
            );
            assert!(parent.editor_open());
            picker.popdown();
            nested_editor.focus();
            assert!(press_escape(&popup));
            drain();
            assert!(!parent.editor_open());
            assert!(anchor.is_sensitive());
            assert!(popup.is_mapped());
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
            edit.execute(&mut state).unwrap();
            parent.test_saved_editor(crate::model::session::SavedItem::Event(Box::new(
                Event::load(&state, &item).unwrap(),
            )));
            assert!(stack.is_transition_running());
            drain();
            let mut options = EditorOptions::load(&state).unwrap();
            options.locale = locale.clone();
            parent.test_existing_editor(
                options,
                UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap(),
            );
            assert!(stack.is_transition_running());
            assert!(
                press_escape(&popup),
                "Escape also cancels while the editor is expanding"
            );
            assert!(!parent.editor_open());
            assert!(anchor.is_sensitive());
            drain();
            assert!(popup.is_mapped());
            gtk::Settings::default()
                .unwrap()
                .set_gtk_enable_animations(false);
            let mut options = EditorOptions::load(&state).unwrap();
            options.locale = locale.clone();
            parent.test_existing_editor(
                options,
                UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap(),
            );
            assert!(!stack.is_transition_running());
            assert!(press_escape(&popup));
            assert!(!parent.editor_open());
            assert_eq!(stack.pages().n_items(), 1);
            assert_eq!(closed.get(), 0);
            parent.remove(&anchor);
            controller.allocate(parent.upcast_ref());
            drain();
            assert!(popup.is_mapped());
            assert_ne!(stack.visible_child().as_ref(), Some(&original));
            clicked.set(false);
            click_edit(
                popup.child().unwrap().upcast_ref(),
                locale.translate("Edit"),
            );
            assert!(clicked.get());
            controller.close();
            drain();
            parent.append(&anchor);
            // Dismissal is independent of the locale and date type checked above.
            if !all_day {
                for dismissal in [
                    "escape",
                    "escape_transition",
                    "click",
                    "saving",
                    "popdown",
                    "hide",
                ] {
                    gtk::Settings::default()
                        .unwrap()
                        .set_gtk_enable_animations(dismissal.starts_with("escape"));
                    window.present();
                    anchor.grab_focus();
                    wait_until(&|| window.is_active());
                    assert!(
                        window.is_active(),
                        "Eventix must be active before opening a popup"
                    );
                    let event = Event::load(&state, &item).unwrap();
                    let popup = EventDetails::new(&event, locale.as_ref());
                    controller.show_details(
                        anchor.upcast_ref(),
                        anchor.upcast_ref(),
                        popup.clone(),
                    );
                    drain();
                    assert!(
                        popup.is_mapped(),
                        "details must open before {dismissal}, all_day={all_day}"
                    );
                    let mut options = EditorOptions::load(&state).unwrap();
                    options.locale = locale.clone();
                    let editor = parent.test_existing_editor(
                        options,
                        UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap(),
                    );
                    editor.summary.set_text("Discarded draft");
                    drain();
                    assert!(
                        popup.is_mapped(),
                        "editor must open before {dismissal}, all_day={all_day}"
                    );
                    assert!(!popup.is_autohide());
                    assert!(anchor.is_sensitive());
                    crate::window::dismiss_popups_on_surface(
                        &window,
                        &parent,
                        &popup.surface().unwrap(),
                    );
                    assert!(
                        parent.editor_open(),
                        "clicking inside the editor keeps it open"
                    );
                    let other_window = gtk::Window::builder()
                        .title("Other window")
                        .default_width(200)
                        .default_height(100)
                        .build();
                    other_window.present();
                    wait_until(&|| !window.is_active());
                    assert!(!window.is_active());
                    crate::window::dismiss_popups_on_surface(
                        &window,
                        &parent,
                        &other_window.surface().unwrap(),
                    );
                    drain();
                    assert!(
                        !popup.is_mapped(),
                        "the popup must not float above the newly activated window"
                    );
                    assert!(parent.editor_open());
                    assert_eq!(editor.summary.text(), "Discarded draft");
                    other_window.close();
                    window.present();
                    wait_until(&|| popup.is_mapped());
                    assert_eq!(controller.current().as_ref(), Some(&popup));
                    assert!(popup.is_mapped(), "returning to Eventix restores its popup");
                    if dismissal == "click" {
                        window.minimize();
                        drain();
                        assert!(!popup.is_mapped(), "minimizing must hide the native popup");
                        assert!(
                            parent.editor_open(),
                            "minimizing must retain the edit draft"
                        );
                        controller.allocate(parent.upcast_ref());
                        assert!(!popup.is_mapped(), "layout must not reshow a hidden popup");
                        window.unminimize();
                        // Wayland has no unminimize request; remap this test window to restore it
                        // without requiring a real taskbar click or activation token.
                        window.set_visible(false);
                        window.present();
                        wait_until(&|| popup.is_mapped());
                        assert!(popup.is_mapped());
                        assert_eq!(editor.summary.text(), "Discarded draft");
                    }
                    match dismissal {
                        "escape" | "escape_transition" => {
                            assert!(press_escape(&popup));
                            assert!(!parent.editor_open());
                            if dismissal == "escape" {
                                drain();
                            }
                            assert!(
                                press_escape(&popup),
                                "the second Escape must close details before GTK handles it"
                            );
                            assert!(controller.current().is_none());
                        }
                        "hide" => window.set_visible(false),
                        "popdown" => popup.popdown(),
                        _ => {
                            editor.set_saving(dismissal == "saving");
                            crate::window::dismiss_popups_on_surface(
                                &window,
                                &parent,
                                &window.surface().unwrap(),
                            );
                        }
                    }
                    drain();
                    if dismissal == "hide" {
                        assert!(!popup.is_mapped());
                        assert!(parent.editor_open());
                        window.present();
                        drain();
                        assert!(popup.is_mapped());
                        assert_eq!(editor.summary.text(), "Discarded draft");
                        parent.dismiss_popovers();
                        drain();
                    }
                    assert!(!popup.is_mapped(), "{dismissal} must dismiss the editor");
                    assert!(!parent.editor_open(), "{dismissal} must clear editor state");
                    assert!(controller.current().is_none());
                    assert_eq!(Event::load(&state, &item).unwrap().summary, "Saved");
                    window.present();
                    drain();
                    controller.allocate(parent.upcast_ref());
                    assert!(!popup.is_mapped(), "dismissed drafts must not reappear");
                }
                for dismissal in ["click", "cancel", "escape"] {
                    let mut options = EditorOptions::load(&state).unwrap();
                    options.locale = locale.clone();
                    let editor = parent.test_new_editor(options);
                    editor.summary.set_text("Unsaved new event");
                    drain();
                    let popup = controller.current().unwrap();
                    assert!(popup.is_mapped());
                    assert!(!popup.is_autohide());
                    assert!(!popup.has_arrow(), "new events use a centered popup");
                    assert!(parent.is_sensitive());
                    assert!(anchor.is_sensitive());
                    crate::window::dismiss_popups_on_surface(
                        &window,
                        &parent,
                        &popup.surface().unwrap(),
                    );
                    assert!(parent.editor_open(), "editor controls must not dismiss it");
                    if dismissal == "click" {
                        editor.summary.set_text("");
                        editor.summary.grab_focus();
                        editor.location.grab_focus();
                        drain();
                        assert!(editor.summary.has_css_class("ev_editor_invalid"));
                        editor.summary.set_text("Unsaved new event");
                        editor.summary.grab_focus();
                        drain();
                        assert!(!editor.summary.has_css_class("ev_editor_invalid"));

                        editor.sections[0].toggle.set_active(true);
                        drain();
                        let picker = editor
                            .start
                            .widget
                            .last_child()
                            .and_downcast::<gtk::MenuButton>()
                            .unwrap();
                        picker.popup();
                        drain();
                        assert!(picker.popover().unwrap().is_mapped());
                        assert!(!press_escape(&popup));
                        assert!(parent.editor_open());
                        picker.popdown();
                        editor.focus();
                    }

                    let other_window = gtk::Window::builder()
                        .default_width(200)
                        .default_height(100)
                        .build();
                    other_window.present();
                    wait_until(&|| !window.is_active());
                    drain();
                    assert!(!popup.is_mapped());
                    assert!(parent.editor_open());
                    assert_eq!(editor.summary.text(), "Unsaved new event");
                    other_window.close();
                    window.present();
                    wait_until(&|| popup.is_mapped());
                    assert!(popup.is_mapped());
                    assert_eq!(controller.current().as_ref(), Some(&popup));

                    match dismissal {
                        "cancel" => editor.cancel.emit_clicked(),
                        "escape" => assert!(press_escape(&popup)),
                        _ => crate::window::dismiss_popups_on_surface(
                            &window,
                            &parent,
                            &window.surface().unwrap(),
                        ),
                    }
                    drain();
                    assert!(!parent.editor_open());
                    assert!(controller.current().is_none());
                    assert!(
                        !popup.is_mapped(),
                        "new-event dismissal must leave no details open"
                    );
                    assert!(parent.is_sensitive());
                }
            }
            gtk::Settings::default()
                .unwrap()
                .set_gtk_enable_animations(true);
            window.close();
            drain();
        }
    }
}
