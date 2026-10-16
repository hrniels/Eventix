// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use anyhow::{Context, bail};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use eventix_ical::objects::{
    CalAction, CalAlarm, CalAttendee, CalDate, CalDateType, CalRelated, CalRole, CalTrigger,
    DateContext,
};
use eventix_locale::Locale;
use eventix_state::actions::items::CreateEvent;
use eventix_view::editor::EditorOptions;
use gtk::{gdk, glib, prelude::*};

mod completion;
mod layout;
mod recurrence;
use layout::{FormSection, action_button, body_label, calendar_factory, choices, row, segments};
use recurrence::RecurrenceInput;

/// Owns the draft independently of calendar refreshes and the details popup.
pub(crate) struct ItemEditor {
    pub layer: gtk::Box,
    pub save: gtk::Button,
    pub cancel: gtk::Button,
    pub id: u64,
    pub saving: Cell<bool>,
    form: gtk::Box,
    error: gtk::Label,
    options: EditorOptions,
    calendar: gtk::DropDown,
    summary: gtk::Entry,
    location: gtk::Entry,
    description: gtk::TextView,
    start: DateInput,
    end: DateInput,
    start_time: gtk::Entry,
    end_time: gtk::Entry,
    all_day: gtk::CheckButton,
    timezone: gtk::DropDown,
    alarm: AlarmInput,
    personal_alarm: AlarmInput,
    personal_override: gtk::CheckButton,
    alarm_mode: gtk::DropDown,
    recurrence: RecurrenceInput,
    attendees: Rc<RefCell<Vec<AttendeeInput>>>,
    new_attendee: gtk::Entry,
    previews: Vec<gtk::Label>,
    sections: Vec<FormSection>,
    _completions: Vec<completion::Completion>,
}

impl ItemEditor {
    pub fn new(id: u64, options: EditorOptions) -> Rc<Self> {
        let editor = Rc::new(Self::build(id, options));
        editor.update_previews();
        editor.connect_previews();
        editor
    }

    fn build(id: u64, options: EditorOptions) -> Self {
        let locale = options.locale.as_ref();
        let form = gtk::Box::new(gtk::Orientation::Vertical, 0);
        form.add_css_class("ev_editor_form");
        let calendar = Self::create_calendar(&options);
        row(&form, locale, "Calendar", &calendar);
        let summary = gtk::Entry::new();
        row(&form, locale, "Summary", &summary);
        let location = gtk::Entry::new();
        row(&form, locale, "Location", &location);
        let location_completion = completion::Completion::new(&location, options.locations.clone());

        let dates = DateSection::new(&form, &options);
        let reminders = ReminderSection::new(&form, &options, &calendar);
        let (description, description_section) = Self::create_description(&form, locale);
        let recurrence = RecurrenceInput::new(locale, options.start.date());
        let repeat_section = FormSection::new(
            &form,
            locale,
            "Repeat",
            &recurrence.widget,
            Some(recurrence.segments.upcast_ref()),
        );

        let attendees = AttendeeSection::new(&form, &options);
        let shell = EditorShell::new(&form, &options);
        let sections = vec![
            dates.section,
            reminders.section,
            description_section,
            repeat_section,
            attendees.section,
        ];
        Self::connect_sections(&sections);
        let previews = sections
            .iter()
            .map(|section| section.preview.clone())
            .collect();
        Self {
            layer: shell.layer,
            save: shell.save,
            cancel: shell.cancel,
            id,
            saving: Cell::new(false),
            form,
            error: shell.error,
            options,
            calendar,
            summary,
            location,
            description,
            start: dates.start,
            end: dates.end,
            start_time: dates.start_time,
            end_time: dates.end_time,
            all_day: dates.all_day,
            timezone: dates.timezone,
            alarm: reminders.alarm,
            personal_alarm: reminders.personal_alarm,
            personal_override: reminders.personal_override,
            alarm_mode: reminders.mode,
            recurrence,
            attendees: attendees.entries,
            new_attendee: attendees.new_entry,
            previews,
            sections,
            _completions: vec![location_completion, attendees.completion],
        }
    }

    fn connect_previews(self: &Rc<Self>) {
        watch_fields(self.form.upcast_ref(), self);
        // Selection models for segmented controls and hidden stack pages are not widget children.
        for selection in [
            &self.recurrence.frequency,
            &self.alarm_mode,
            &self.alarm.trigger,
            &self.personal_alarm.trigger,
        ] {
            let weak = Rc::downgrade(self);
            selection.connect_selected_notify(move |_| {
                if let Some(editor) = weak.upgrade() {
                    editor.update_previews();
                }
            });
        }
        watch_fields(self.alarm.widget.upcast_ref(), self);
        watch_fields(self.personal_alarm.widget.upcast_ref(), self);
    }

    fn connect_sections(sections: &[FormSection]) {
        for section in sections {
            let others: Vec<_> = sections
                .iter()
                .filter(|other| other.toggle != section.toggle)
                .map(|other| other.toggle.downgrade())
                .collect();
            section.toggle.connect_toggled(move |button| {
                if button.is_active() {
                    for other in &others {
                        if let Some(other) = other.upgrade() {
                            other.set_active(false);
                        }
                    }
                }
            });
        }
    }

    fn create_calendar(options: &EditorOptions) -> gtk::DropDown {
        let calendar = gtk::DropDown::from_strings(
            &options
                .calendars
                .iter()
                .map(|cal| cal.name.as_str())
                .collect::<Vec<_>>(),
        );
        let factory = calendar_factory(&options.calendars);
        calendar.set_factory(Some(&factory));
        calendar.set_list_factory(Some(&factory));
        if let Some(index) = options
            .calendars
            .iter()
            .position(|cal| Some(&cal.id) == options.last_calendar.as_ref())
        {
            calendar.set_selected(index as u32);
        }
        calendar
    }

    fn create_description(form: &gtk::Box, locale: &dyn Locale) -> (gtk::TextView, FormSection) {
        let description = gtk::TextView::new();
        description.set_wrap_mode(gtk::WrapMode::WordChar);
        description.set_top_margin(6);
        description.set_bottom_margin(6);
        description.set_left_margin(6);
        description.set_right_margin(6);
        let scroll = gtk::ScrolledWindow::builder()
            .min_content_height(100)
            .max_content_height(180)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&description)
            .build();
        let section = FormSection::new(form, locale, "Description", &scroll, None);
        (description, section)
    }

    pub fn focus(&self) {
        self.summary.grab_focus();
    }

    fn update_previews(&self) {
        self.update_date_preview();
        self.update_alarm_preview();
        self.update_description_preview();
        self.update_recurrence_preview();
        self.update_attendees_preview();
        for preview in &self.previews {
            preview
                .set_tooltip_text((!preview.text().is_empty()).then_some(preview.text().as_str()));
        }
    }

    fn update_date_preview(&self) {
        let locale = self.options.locale.as_ref();
        let start = self.start.entry.text();
        let end = self.end.entry.text();
        self.previews[0].set_text(&if self.all_day.is_active() {
            format!("{start} – {end} · {}", locale.translate("All day"))
        } else {
            format!(
                "{start} {} – {end} {}, {}",
                self.start_time.text(),
                self.end_time.text(),
                chrono_tz::TZ_VARIANTS
                    .get(self.timezone.selected() as usize)
                    .map_or("", |zone| zone.name()),
            )
        });
    }

    fn update_alarm_preview(&self) {
        let locale = self.options.locale.as_ref();
        let timezone = chrono_tz::TZ_VARIANTS
            .get(self.timezone.selected() as usize)
            .unwrap_or(locale.timezone());
        let (label, summary) = if self.alarm_mode.selected() == 1 {
            (
                "Personal",
                if self.personal_override.is_active() {
                    self.personal_alarm.summary(timezone, locale)
                } else {
                    locale.translate("Default").to_string()
                },
            )
        } else {
            ("Calendar", self.alarm.summary(timezone, locale))
        };
        self.previews[1].set_text(&format!("{}: {summary}", locale.translate(label)));
    }

    fn update_description_preview(&self) {
        let buffer = self.description.buffer();
        self.previews[2].set_text(
            &buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), true)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        );
    }

    fn update_recurrence_preview(&self) {
        let locale = self.options.locale.as_ref();
        self.previews[3].set_text(
            locale.translate(match self.recurrence.frequency.selected() {
                1 => "Hourly",
                2 => "Daily",
                3 => "Weekly",
                4 => "Monthly",
                5 => "Yearly",
                _ => "None",
            }),
        );
    }

    fn update_attendees_preview(&self) {
        let locale = self.options.locale.as_ref();
        let attendees = attendee_names(&self.attendees.borrow());
        self.previews[4].set_text(if attendees.is_empty() {
            locale.translate("None")
        } else {
            &attendees
        });
    }

    pub fn set_saving(&self, saving: bool) {
        self.saving.set(saving);
        self.form.set_sensitive(!saving);
        self.save.set_sensitive(!saving);
        self.cancel.set_sensitive(!saving);
        for section in &self.sections {
            section.toggle.set_sensitive(!saving);
        }
    }

    pub fn show_error(&self, message: &str) {
        self.error.set_text(message);
        self.error.set_visible(true);
    }

    pub fn command(&self) -> anyhow::Result<Box<CreateEvent>> {
        let locale = self.options.locale.as_ref();
        self.validate_text_fields()?;
        let calendar = self
            .options
            .calendars
            .get(self.calendar.selected() as usize)
            .context(locale.translate("error.create_calendar_first").to_string())?;
        let timezone = chrono_tz::TZ_VARIANTS
            .get(self.timezone.selected() as usize)
            .context(locale.translate("error.valid_date_time").to_string())?;
        let (start, end) = self.event_dates(timezone)?;
        let recurrence = self.recurrence.value(&start, timezone, locale)?;
        let alarms = self.alarm.value(timezone, locale)?;
        let personal_alarms = if calendar.personal && self.personal_override.is_active() {
            Some(self.personal_alarm.value(timezone, locale)?)
        } else {
            None
        };
        let attendees = self
            .attendees
            .borrow()
            .iter()
            .filter(|attendee| attendee.widget.get_visible())
            .map(AttendeeInput::value)
            .collect();
        let buffer = self.description.buffer();
        Ok(Box::new(CreateEvent {
            calendar: calendar.id.clone(),
            summary: self.summary.text().to_string(),
            location: self.location.text().to_string(),
            description: buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), true)
                .to_string(),
            start,
            end,
            recurrence,
            alarms,
            personal_alarms,
            attendees,
        }))
    }

    fn validate_text_fields(&self) -> anyhow::Result<()> {
        let locale = self.options.locale.as_ref();
        if self.summary.text().trim().is_empty() {
            bail!(locale.translate("error.summary_empty").to_string());
        }
        if !self.new_attendee.text().trim().is_empty() {
            bail!(locale.translate("error.new_attendee_not_empty").to_string());
        }
        Ok(())
    }

    fn event_dates(&self, timezone: &chrono_tz::Tz) -> anyhow::Result<(CalDate, CalDate)> {
        let locale = self.options.locale.as_ref();
        let start_date = self.start.date(locale)?;
        let end_date = self.end.date(locale)?;
        let (start, end) = if self.all_day.is_active() {
            (
                CalDate::new_date(start_date, CalDateType::Inclusive),
                CalDate::new_date(
                    end_date
                        .succ_opt()
                        .context(locale.translate("error.valid_date_time").to_string())?,
                    CalDateType::Exclusive,
                ),
            )
        } else {
            (
                CalDate::new_datetime(
                    start_date,
                    Some(parse_time(&self.start_time, locale)?),
                    timezone.name(),
                ),
                CalDate::new_datetime(
                    end_date,
                    Some(parse_time(&self.end_time, locale)?),
                    timezone.name(),
                ),
            )
        };
        if end_date < start_date || start > end {
            bail!(locale.translate("error.end_before_start").to_string());
        }
        Ok((start, end))
    }
}

struct EditorShell {
    layer: gtk::Box,
    save: gtk::Button,
    cancel: gtk::Button,
    error: gtk::Label,
}

impl EditorShell {
    fn new(form: &gtk::Box, options: &EditorOptions) -> Self {
        let locale = options.locale.as_ref();
        let layer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        layer.set_hexpand(true);
        layer.set_vexpand(true);
        layer.set_halign(gtk::Align::Fill);
        layer.set_valign(gtk::Align::Fill);
        layer.add_css_class("ev_editor_backdrop");
        let panel = Self::create_panel(locale);
        let scroll = gtk::ScrolledWindow::builder()
            .child(form)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(640)
            .build();
        panel.append(&scroll);
        let error = Self::create_error();
        panel.append(&error);
        let (save, cancel) = Self::append_actions(&panel, locale);
        layer.append(&panel);
        if options.calendars.is_empty() {
            error.set_text(locale.translate("error.create_calendar_first"));
            error.set_visible(true);
            save.set_sensitive(false);
        }
        Self {
            layer,
            save,
            cancel,
            error,
        }
    }

    fn create_panel(locale: &dyn Locale) -> gtk::Box {
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panel.add_css_class("ev_item_editor");
        panel.set_halign(gtk::Align::Center);
        panel.set_valign(gtk::Align::Center);
        panel.set_vexpand(true);
        // The scroller's natural size is bounded; its minimum can shrink with the app.
        panel.set_size_request(684, -1);
        let title = gtk::Label::new(Some(locale.translate("New Event")));
        title.add_css_class("ev_editor_title");
        let header = gtk::HeaderBar::new();
        header.set_show_title_buttons(false);
        header.set_title_widget(Some(&title));
        panel.append(&header);
        panel
    }

    fn create_error() -> gtk::Label {
        let error = gtk::Label::new(None);
        error.set_wrap(true);
        error.set_xalign(0.0);
        error.set_visible(false);
        error.add_css_class("ev_editor_error");
        error
    }

    fn append_actions(panel: &gtk::Box, locale: &dyn Locale) -> (gtk::Button, gtk::Button) {
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.add_css_class("ev_editor_actions");
        actions.set_halign(gtk::Align::End);
        let save = action_button(locale, "Save", "floppy2-fill");
        let cancel = action_button(locale, "Cancel", "x-circle");
        actions.append(&save);
        actions.append(&cancel);
        panel.append(&actions);
        (save, cancel)
    }
}

struct DateSection {
    section: FormSection,
    start: DateInput,
    end: DateInput,
    start_time: gtk::Entry,
    end_time: gtk::Entry,
    all_day: gtk::CheckButton,
    timezone: gtk::DropDown,
}

impl DateSection {
    fn new(form: &gtk::Box, options: &EditorOptions) -> Self {
        let locale = options.locale.as_ref();
        let start = DateInput::new(locale, options.start.date());
        let end_value = options.start + Duration::hours(1);
        let end = DateInput::new(locale, end_value.date());
        let start_time = time_entry(options.start.time());
        let end_time = time_entry(end_value.time());
        let all_day = gtk::CheckButton::new();
        all_day.set_halign(gtk::Align::Start);
        let timezone = Self::create_timezone(locale);
        let left = Self::create_column(locale, "Start", &start, &start_time);
        let right = Self::create_column(locale, "End", &end, &end_time);
        Self::append_field(&left, locale, 1, "All day", all_day.upcast_ref());
        Self::append_field(&right, locale, 1, "Timezone", timezone.upcast_ref());
        let dates = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        dates.set_homogeneous(true);
        dates.append(&left);
        dates.append(&right);
        Self::connect_all_day(&all_day, &start_time, &end_time, &timezone);
        let section = FormSection::new(form, locale, "Date", &dates, None);
        Self {
            section,
            start,
            end,
            start_time,
            end_time,
            all_day,
            timezone,
        }
    }

    fn create_column(
        locale: &dyn Locale,
        name: &str,
        date: &DateInput,
        time: &gtk::Entry,
    ) -> gtk::Grid {
        let grid = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(8)
            .build();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&date.widget);
        row.append(time);
        Self::append_field(&grid, locale, 0, name, row.upcast_ref());
        grid
    }

    fn append_field(
        grid: &gtk::Grid,
        locale: &dyn Locale,
        index: i32,
        name: &str,
        field: &gtk::Widget,
    ) {
        let label = body_label(locale, name);
        label.set_text(&format!("{}:", locale.translate(name)));
        label.set_width_chars(8);
        label.set_mnemonic_widget(Some(field));
        grid.attach(&label, 0, index, 1, 1);
        field.set_hexpand(true);
        grid.attach(field, 1, index, 1, 1);
    }

    fn create_timezone(locale: &dyn Locale) -> gtk::DropDown {
        let zones: Vec<_> = chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name()).collect();
        let timezone = gtk::DropDown::from_strings(&zones);
        timezone.set_expression(Some(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            None::<gtk::Expression>,
            "string",
        )));
        // GTK 4.12 adds substring matching; retain native prefix search on GTK 4.10.
        if timezone.find_property("search-match-mode").is_some() {
            timezone.set_property("search-match-mode", gtk::StringFilterMatchMode::Substring);
        }
        timezone.set_enable_search(true);
        if let Some(index) = zones
            .iter()
            .position(|name| *name == locale.timezone().name())
        {
            timezone.set_selected(index as u32);
        }
        timezone
    }

    fn connect_all_day(
        all_day: &gtk::CheckButton,
        start_time: &gtk::Entry,
        end_time: &gtk::Entry,
        timezone: &gtk::DropDown,
    ) {
        let times = [start_time.clone(), end_time.clone()];
        let zone = timezone.clone();
        all_day.connect_toggled(move |button| {
            for time in &times {
                time.set_sensitive(!button.is_active());
            }
            zone.set_sensitive(!button.is_active());
        });
    }
}

struct ReminderSection {
    section: FormSection,
    alarm: AlarmInput,
    personal_alarm: AlarmInput,
    personal_override: gtk::CheckButton,
    mode: gtk::DropDown,
}

impl ReminderSection {
    fn new(form: &gtk::Box, options: &EditorOptions, calendar: &gtk::DropDown) -> Self {
        let locale = options.locale.as_ref();
        let alarms = gtk::Stack::new();
        alarms.set_hhomogeneous(false);
        alarms.set_vhomogeneous(false);
        let alarm = AlarmInput::new(locale, options.start.date(), options.start.time());
        alarms.add_named(&alarm.widget, Some("calendar"));
        let (personal_alarm, personal_override) = Self::append_personal(&alarms, options);
        let mode = dropdown(locale, &["Calendar", "Personal"]);
        let modes = segments(locale, &mode, &["Calendar", "Personal"]);
        let panels = alarms.clone();
        mode.connect_selected_notify(move |mode| {
            panels.set_visible_child_name(if mode.selected() == 1 {
                "personal"
            } else {
                "calendar"
            });
        });
        Self::connect_availability(calendar, &mode, &modes, options);
        let section = FormSection::new(form, locale, "Reminder", &alarms, Some(modes.upcast_ref()));
        Self {
            section,
            alarm,
            personal_alarm,
            personal_override,
            mode,
        }
    }

    fn append_personal(
        alarms: &gtk::Stack,
        options: &EditorOptions,
    ) -> (AlarmInput, gtk::CheckButton) {
        let locale = options.locale.as_ref();
        let personal_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let overwrite = gtk::CheckButton::with_label(locale.translate("Overwrite default"));
        personal_box.append(&overwrite);
        let alarm = AlarmInput::new(locale, options.start.date(), options.start.time());
        alarm.widget.set_sensitive(false);
        personal_box.append(&alarm.widget);
        let personal = alarm.widget.clone();
        overwrite.connect_toggled(move |button| personal.set_sensitive(button.is_active()));
        alarms.add_named(&personal_box, Some("personal"));
        (alarm, overwrite)
    }

    fn connect_availability(
        calendar: &gtk::DropDown,
        mode: &gtk::DropDown,
        modes: &gtk::Box,
        options: &EditorOptions,
    ) {
        let available: Vec<_> = options.calendars.iter().map(|cal| cal.personal).collect();
        let personal_mode = modes.last_child().unwrap();
        personal_mode.set_sensitive(
            available
                .get(calendar.selected() as usize)
                .copied()
                .unwrap_or(false),
        );
        let mode = mode.clone();
        calendar.connect_selected_notify(move |combo| {
            let personal = available
                .get(combo.selected() as usize)
                .copied()
                .unwrap_or(false);
            personal_mode.set_sensitive(personal);
            if !personal {
                mode.set_selected(0);
            }
        });
    }
}

struct AttendeeSection {
    section: FormSection,
    entries: Rc<RefCell<Vec<AttendeeInput>>>,
    new_entry: gtk::Entry,
    completion: completion::Completion,
}

impl AttendeeSection {
    fn new(form: &gtk::Box, options: &EditorOptions) -> Self {
        let locale = options.locale.as_ref();
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
        widget.append(&rows);
        let new_entry = gtk::Entry::new();
        new_entry.set_placeholder_text(Some(locale.translate("New attendee")));
        new_entry.set_hexpand(true);
        let completion = completion::Completion::new(&new_entry, options.contacts.clone());
        let add = gtk::Button::with_label(locale.translate("Add"));
        let add_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        add_row.append(&new_entry);
        add_row.append(&add);
        widget.append(&add_row);
        let section = FormSection::new(form, locale, "Attendees", &widget, None);
        let section = Self {
            section,
            entries: Rc::new(RefCell::new(Vec::new())),
            new_entry,
            completion,
        };
        section.connect_add(&add, rows, options);
        section
    }

    fn connect_add(&self, add: &gtk::Button, rows: gtk::Box, options: &EditorOptions) {
        let entries = self.entries.clone();
        let preview = self.section.preview.downgrade();
        let entry = self.new_entry.clone();
        let locale = options.locale.clone();
        add.connect_clicked(move |_| {
            let value = entry.text().trim().to_string();
            if value.is_empty() {
                return;
            }
            let input = AttendeeInput::new(&value, locale.as_ref());
            Self::connect_removal_preview(
                &input,
                &entries,
                preview.clone(),
                locale.translate("None").to_string(),
            );
            rows.append(&input.widget);
            entries.borrow_mut().push(input);
            entry.set_text("");
        });
        let weak_add = add.downgrade();
        self.new_entry.connect_activate(move |_| {
            if let Some(add) = weak_add.upgrade() {
                add.emit_clicked();
            }
        });
    }

    fn connect_removal_preview(
        input: &AttendeeInput,
        entries: &Rc<RefCell<Vec<AttendeeInput>>>,
        preview: glib::WeakRef<gtk::Label>,
        empty: String,
    ) {
        let weak = Rc::downgrade(entries);
        input.widget.connect_visible_notify(move |_| {
            if let (Some(entries), Some(preview)) = (weak.upgrade(), preview.upgrade()) {
                let names = attendee_names(&entries.borrow());
                preview.set_text(if names.is_empty() { &empty } else { &names });
            }
        });
    }
}

fn attendee_names(entries: &[AttendeeInput]) -> String {
    entries
        .iter()
        .filter(|entry| entry.widget.get_visible())
        .map(AttendeeInput::short_name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Keeps collapsed summaries current without giving signal handlers ownership of the draft.
fn watch_fields(widget: &gtk::Widget, editor: &Rc<ItemEditor>) {
    for property in ["text", "selected", "active", "value"] {
        if widget.find_property(property).is_some() && !widget.is::<gtk::Label>() {
            let weak = Rc::downgrade(editor);
            widget.connect_notify_local(Some(property), move |_, _| {
                if let Some(editor) = weak.upgrade() {
                    editor.update_previews();
                }
            });
        }
    }
    if let Some(text) = widget.downcast_ref::<gtk::TextView>() {
        let weak = Rc::downgrade(editor);
        text.buffer().connect_changed(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.update_previews();
            }
        });
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        watch_fields(&widget, editor);
        child = widget.next_sibling();
    }
}

fn dropdown(locale: &dyn Locale, values: &[&str]) -> gtk::DropDown {
    let names: Vec<_> = values.iter().map(|value| locale.translate(value)).collect();
    gtk::DropDown::from_strings(&names)
}

fn spin(min: f64, max: f64, value: f64) -> gtk::SpinButton {
    let widget = gtk::SpinButton::with_range(min, max, 1.0);
    widget.set_value(value);
    widget.set_numeric(true);
    widget
}

fn time_entry(time: NaiveTime) -> gtk::Entry {
    gtk::Entry::builder()
        .text(time.format("%H:%M").to_string())
        .width_chars(5)
        .max_width_chars(8)
        .build()
}

fn parse_time(entry: &gtk::Entry, locale: &dyn Locale) -> anyhow::Result<NaiveTime> {
    NaiveTime::parse_from_str(entry.text().as_str(), "%H:%M")
        .map_err(|_| anyhow::anyhow!(locale.translate("error.valid_date_time").to_string()))
}

struct DateInput {
    widget: gtk::Box,
    entry: gtk::Entry,
}

impl DateInput {
    fn new(locale: &dyn Locale, date: NaiveDate) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let entry = gtk::Entry::builder()
            .text(date.to_string())
            .width_chars(10)
            .build();
        entry.set_hexpand(true);
        let calendar = gtk::Calendar::new();
        let picker = Self::create_picker(locale, &calendar);
        let syncing = Rc::new(Cell::new(false));
        Self::connect_picker_open(&entry, &picker, &calendar, syncing.clone());
        Self::connect_day_selected(&entry, &picker, &calendar, syncing);
        widget.append(&entry);
        widget.append(&picker);
        Self { widget, entry }
    }

    fn create_picker(locale: &dyn Locale, calendar: &gtk::Calendar) -> gtk::MenuButton {
        let picker = gtk::MenuButton::builder()
            .icon_name("x-office-calendar-symbolic")
            .build();
        picker.set_tooltip_text(Some(locale.translate("Choose date")));
        let popover = gtk::Popover::new();
        popover.set_child(Some(calendar));
        picker.set_popover(Some(&popover));
        picker
    }

    fn connect_picker_open(
        entry: &gtk::Entry,
        picker: &gtk::MenuButton,
        calendar: &gtk::Calendar,
        syncing: Rc<Cell<bool>>,
    ) {
        let input = entry.clone();
        let cal = calendar.clone();
        picker.connect_active_notify(move |picker| {
            if picker.is_active()
                && let Ok(date) = NaiveDate::parse_from_str(input.text().as_str(), "%Y-%m-%d")
                && let Ok(date) = glib::DateTime::from_local(
                    date.year(),
                    date.month() as i32,
                    date.day() as i32,
                    12,
                    0,
                    0.0,
                )
            {
                syncing.set(true);
                cal.select_day(&date);
                syncing.set(false);
            }
        });
    }

    fn connect_day_selected(
        entry: &gtk::Entry,
        picker: &gtk::MenuButton,
        calendar: &gtk::Calendar,
        syncing: Rc<Cell<bool>>,
    ) {
        let input = entry.clone();
        let button = picker.downgrade();
        calendar.connect_day_selected(move |calendar| {
            if syncing.get() {
                return;
            }
            let date = calendar.date();
            input.set_text(&format!(
                "{:04}-{:02}-{:02}",
                date.year(),
                date.month(),
                date.day_of_month()
            ));
            if let Some(button) = button.upgrade() {
                button.popdown();
            }
        });
    }

    fn date(&self, locale: &dyn Locale) -> anyhow::Result<NaiveDate> {
        NaiveDate::parse_from_str(self.entry.text().as_str(), "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!(locale.translate("error.valid_date_time").to_string()))
    }
}

struct AlarmInput {
    widget: gtk::Box,
    trigger: gtk::DropDown,
    duration: gtk::SpinButton,
    unit: gtk::DropDown,
    relation: gtk::DropDown,
    date: DateInput,
    time: gtk::Entry,
}

impl AlarmInput {
    fn new(locale: &dyn Locale, date: NaiveDate, time: NaiveTime) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let trigger = dropdown(locale, &["None", "Relative", "Absolute"]);
        let buttons = choices(locale, &trigger, &["None", "Relative", "Absolute"]);
        widget.append(&buttons[0]);
        let (relative, duration, unit, relation) = Self::create_relative(locale);
        Self::append_mode_row(&widget, &buttons[1], &relative);
        let (absolute, date, time) = Self::create_absolute(locale, date, time);
        Self::append_mode_row(&widget, &buttons[2], &absolute);
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
            date,
            time,
        }
    }

    fn create_relative(
        locale: &dyn Locale,
    ) -> (gtk::Box, gtk::SpinButton, gtk::DropDown, gtk::DropDown) {
        let relative = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let duration = spin(0.0, 100000.0, 15.0);
        let unit = dropdown(locale, &["Minutes", "Hours", "Days"]);
        let relation = dropdown(
            locale,
            &["BeforeStart", "AfterStart", "BeforeEnd", "AfterEnd"],
        );
        relative.append(&duration);
        relative.append(&unit);
        relative.append(&relation);
        relative.set_sensitive(false);
        (relative, duration, unit, relation)
    }

    fn create_absolute(
        locale: &dyn Locale,
        date: NaiveDate,
        time: NaiveTime,
    ) -> (gtk::Box, DateInput, gtk::Entry) {
        let absolute = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let date = DateInput::new(locale, date);
        date.widget.set_hexpand(false);
        absolute.set_hexpand(false);
        absolute.set_halign(gtk::Align::Start);
        let time = time_entry(time);
        absolute.append(&date.widget);
        absolute.append(&time);
        absolute.set_sensitive(false);
        (absolute, date, time)
    }

    fn append_mode_row(widget: &gtk::Box, button: &gtk::CheckButton, controls: &gtk::Box) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        button.set_size_request(100, -1);
        row.append(button);
        row.append(controls);
        widget.append(&row);
    }

    fn summary(&self, timezone: &chrono_tz::Tz, locale: &dyn Locale) -> String {
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

    fn value(
        &self,
        timezone: &chrono_tz::Tz,
        locale: &dyn Locale,
    ) -> anyhow::Result<Vec<CalAlarm>> {
        let trigger = match self.trigger.selected() {
            1 => self.relative_trigger(),
            2 => self.absolute_trigger(timezone, locale)?,
            _ => return Ok(Vec::new()),
        };
        let mut alarm = CalAlarm::new(CalAction::Display, trigger);
        alarm.set_description(Some(locale.translate("Reminder").to_string()));
        Ok(vec![alarm])
    }

    fn relative_trigger(&self) -> CalTrigger {
        let amount = i64::from(self.duration.value_as_int());
        let amount = if self.relation.selected().is_multiple_of(2) {
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
            related: if self.relation.selected() < 2 {
                CalRelated::Start
            } else {
                CalRelated::End
            },
        }
    }

    fn absolute_trigger(
        &self,
        timezone: &chrono_tz::Tz,
        locale: &dyn Locale,
    ) -> anyhow::Result<CalTrigger> {
        let date = CalDate::new_datetime(
            self.date.date(locale)?,
            Some(parse_time(&self.time, locale)?),
            timezone.name(),
        );
        Ok(CalTrigger::Absolute(
            DateContext::system().date_to_utc(&date, timezone),
        ))
    }
}

struct AttendeeInput {
    widget: gtk::Box,
    name: String,
    role: gtk::ToggleButton,
}

impl AttendeeInput {
    fn new(name: &str, locale: &dyn Locale) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let label = gtk::Label::new(Some(name));
        label.set_hexpand(true);
        label.set_xalign(0.0);
        label.set_wrap(true);
        let role = Self::create_role(locale);
        let remove = gtk::Button::with_label(locale.translate("Remove"));
        widget.append(&label);
        widget.append(&role);
        widget.append(&remove);
        let weak = widget.downgrade();
        remove.connect_clicked(move |_| {
            if let Some(widget) = weak.upgrade() {
                widget.set_visible(false);
            }
        });
        Self {
            widget,
            name: name.to_string(),
            role,
        }
    }

    fn create_role(locale: &dyn Locale) -> gtk::ToggleButton {
        let role = gtk::ToggleButton::new();
        let required = locale.translate("Required").to_string();
        let optional = locale.translate("Optional").to_string();
        role.set_child(Some(&layout::image("person-fill")));
        role.set_tooltip_text(Some(&required));
        role.connect_toggled(move |button| {
            button.set_child(Some(&layout::image(if button.is_active() {
                "person"
            } else {
                "person-fill"
            })));
            button.set_tooltip_text(Some(if button.is_active() {
                &optional
            } else {
                &required
            }));
        });
        role
    }

    fn short_name(&self) -> &str {
        self.name
            .rsplit_once('<')
            .filter(|(name, address)| !name.trim().is_empty() && address.ends_with('>'))
            .map_or(self.name.as_str(), |(name, _)| name.trim())
    }

    fn value(&self) -> CalAttendee {
        let (name, address) = self
            .name
            .rsplit_once('<')
            .and_then(|(name, address)| {
                address
                    .strip_suffix('>')
                    .map(|address| (Some(name.trim()), address.trim()))
            })
            .unwrap_or((None, self.name.trim()));
        let mut attendee = CalAttendee::new(format!("mailto:{address}"));
        if let Some(name) = name {
            attendee.set_common_name(name.to_string());
        }
        attendee.set_role(if self.role.is_active() {
            CalRole::Optional
        } else {
            CalRole::Required
        });
        attendee
    }
}

/// Gives nested GTK popovers first refusal on Escape.
pub(crate) fn has_nested_popup(window: &gtk::Window) -> bool {
    let mut focused = gtk::prelude::GtkWindowExt::focus(window);
    while let Some(widget) = focused {
        let mut child = widget.first_child();
        while let Some(current) = child {
            if current.is::<gtk::Popover>() && current.is_visible() {
                return true;
            }
            child = current.next_sibling();
        }
        if widget.is::<gtk::Popover>() {
            return true;
        }
        focused = widget.parent();
    }
    false
}

pub(crate) fn is_save_key(key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
    matches!(key, gdk::Key::Return | gdk::Key::KP_Enter)
        && modifiers.contains(gdk::ModifierType::CONTROL_MASK)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDateTime;
    use eventix_ical::objects::CalRRuleFreq;
    use eventix_view::editor::{EditorCalendar, EditorOptions};
    use gtk::prelude::*;

    use super::{CalDate, CalDateType, ItemEditor};

    #[test]
    #[ignore = "requires a GTK display"]
    fn editor_builds_valid_drafts_and_retains_values_after_save_failure() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        for language in [
            eventix_locale::LocaleType::English,
            eventix_locale::LocaleType::German,
        ] {
            let options = EditorOptions {
                calendars: vec![
                    EditorCalendar {
                        id: "test".into(),
                        name: "Private Events".into(),
                        background: "#27c281".into(),
                        personal: true,
                    },
                    EditorCalendar {
                        id: "work".into(),
                        name: "Work".into(),
                        background: "#637591".into(),
                        personal: false,
                    },
                ],
                last_calendar: Some("test".into()),
                locations: std::iter::once("Office".into())
                    .chain(std::iter::once("Office annex".into()))
                    .chain((0..25).map(|index| format!("Office room {index:02}")))
                    .collect(),
                contacts: vec!["Alice <alice@example.org>".into()],
                start: "2026-10-15T10:00:00".parse::<NaiveDateTime>().unwrap(),
                locale: eventix_locale::new_with_timezone(
                    &xdg,
                    language,
                    chrono_tz::Europe::Berlin,
                )
                .unwrap(),
            };
            let editor = ItemEditor::new(1, options);
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
            overlay.add_overlay(&editor.layer);
            let window = gtk::Window::builder()
                .default_width(900)
                .default_height(700)
                .child(&overlay)
                .build();
            window.present();
            let drain = || {
                for _ in 0..20 {
                    while gtk::glib::MainContext::default().iteration(false) {}
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                // Broadway can run without a browser client; allocate the app viewport explicitly
                // so the same layout and snapshot assertions also run in that headless setup.
                overlay.allocate(window.default_width(), window.default_height(), -1, None);
            };
            drain();
            let panel = editor.layer.first_child().unwrap();
            let screenshot = |name: &str| {
                if let Some(directory) = std::env::var_os("EVENTIX_EDITOR_SCREENSHOTS") {
                    let snapshot = gtk::Snapshot::new();
                    editor.layer.snapshot_child(&panel, &snapshot);
                    let node = snapshot.to_node().unwrap();
                    window
                        .renderer()
                        .unwrap()
                        .render_texture(&node, None)
                        .save_to_png(std::path::Path::new(&directory).join(
                            if language == eventix_locale::LocaleType::English {
                                format!("editor-{name}.png")
                            } else {
                                format!("editor-German-{name}.png")
                            },
                        ))
                        .unwrap();
                }
            };
            let bounds = panel.compute_bounds(&overlay).unwrap();
            assert!(
                panel.width() >= 682 && panel.height() > 350,
                "panel: {} × {}",
                panel.width(),
                panel.height()
            );
            assert!(
                (bounds.x() + bounds.width() / 2.0 - overlay.width() as f32 / 2.0).abs() <= 1.0
            );
            assert!(
                (bounds.y() + bounds.height() / 2.0 - overlay.height() as f32 / 2.0).abs() <= 1.0
            );
            screenshot("collapsed");
            let key = |entry: &gtk::Entry, key: gtk::gdk::Key| {
                let controllers = entry.observe_controllers();
                for index in 0..controllers.n_items() {
                    if let Ok(controller) = controllers
                        .item(index)
                        .unwrap()
                        .downcast::<gtk::EventControllerKey>()
                    {
                        let handled = controller.emit_by_name::<bool>(
                            "key-pressed",
                            &[&key, &0u32, &gtk::gdk::ModifierType::empty()],
                        );
                        if handled {
                            return true;
                        }
                    }
                }
                false
            };
            editor.location.grab_focus();
            let original_height = panel.height();
            editor.location.set_text("off");
            drain();
            assert_eq!(panel.height(), original_height);
            assert!(super::has_nested_popup(&window));
            assert!(key(&editor.location, gtk::gdk::Key::Down));
            assert!(key(&editor.location, gtk::gdk::Key::Tab));
            assert_eq!(editor.location.text(), "Office");
            drain();
            assert!(!super::has_nested_popup(&window));
            editor.location.set_text("annex");
            drain();
            let completion = descendant::<gtk::Popover>(editor.location.upcast_ref()).unwrap();
            let short_height = completion.measure(gtk::Orientation::Vertical, -1).1;
            editor.location.set_text("of");
            drain();
            let long_height = completion.measure(gtk::Orientation::Vertical, -1).1;
            assert!(long_height > short_height);
            // Native popup surfaces also need allocation without a Broadway browser client.
            completion.allocate(
                completion.measure(gtk::Orientation::Horizontal, -1).1,
                long_height,
                -1,
                None,
            );
            let results = descendant::<gtk::ListBox>(completion.upcast_ref()).unwrap();
            assert!(results.row_at_index(19).is_some());
            assert!(results.row_at_index(20).is_none());
            for _ in 0..20 {
                assert!(key(&editor.location, gtk::gdk::Key::Down));
            }
            assert_eq!(results.selected_row().unwrap().index(), 19);
            let scroll = descendant::<gtk::ScrolledWindow>(completion.upcast_ref()).unwrap();
            assert!(scroll.vadjustment().value() > 0.0);
            assert!(scroll.measure(gtk::Orientation::Vertical, -1).1 <= 240);
            editor.location.set_text("annex");
            drain();
            assert_eq!(
                completion.measure(gtk::Orientation::Vertical, -1).1,
                short_height
            );
            assert!(key(&editor.location, gtk::gdk::Key::Escape));
            assert!(editor.layer.is_visible());
            // A replacement emits deletion and insertion; only the final query should matter.
            editor.location.set_text("no results");
            drain();
            assert!(!completion.is_visible());
            editor.location.set_text("of");
            drain();
            assert!(completion.is_visible());
            editor.location.set_text("no results");
            editor.location.set_text("of");
            drain();
            assert!(completion.is_visible());
            editor.summary.grab_focus();
            drain();
            assert!(!completion.is_visible());
            editor.location.grab_focus();
            drain();
            assert!(completion.is_visible());
            editor.sections[4].toggle.set_active(true);
            drain();
            editor.new_attendee.grab_focus();
            editor.new_attendee.set_text("ali");
            drain();
            assert!(key(&editor.new_attendee, gtk::gdk::Key::Down));
            assert!(key(&editor.new_attendee, gtk::gdk::Key::Return));
            assert_eq!(editor.new_attendee.text(), "Alice <alice@example.org>");
            editor.new_attendee.set_text("");
            editor.sections[0].toggle.set_active(true);
            assert!(!editor.sections[4].toggle.is_active());
            drain();
            fn descendant<T: IsA<gtk::Widget> + gtk::glib::object::IsClass>(
                widget: &gtk::Widget,
            ) -> Option<T> {
                if let Ok(found) = widget.clone().downcast::<T>() {
                    return Some(found);
                }
                let mut child = widget.first_child();
                while let Some(current) = child {
                    if let Some(found) = descendant::<T>(&current) {
                        return Some(found);
                    }
                    child = current.next_sibling();
                }
                None
            }
            editor.timezone.activate();
            drain();
            let search = descendant::<gtk::SearchEntry>(editor.timezone.upcast_ref()).unwrap();
            search.set_text(
                if editor.timezone.find_property("search-match-mode").is_some() {
                    "Berlin"
                } else {
                    "Europe/Berlin"
                },
            );
            drain();
            let list = descendant::<gtk::ListView>(editor.timezone.upcast_ref()).unwrap();
            assert_eq!(list.model().unwrap().n_items(), 1);
            assert_eq!(
                list.model()
                    .unwrap()
                    .item(0)
                    .unwrap()
                    .downcast::<gtk::StringObject>()
                    .unwrap()
                    .string(),
                "Europe/Berlin"
            );
            descendant::<gtk::Popover>(editor.timezone.upcast_ref())
                .unwrap()
                .popdown();
            screenshot("date");
            editor.sections[0].toggle.set_active(false);
            editor.sections[1].toggle.set_active(true);
            drain();
            screenshot("reminder");
            editor.sections[1].toggle.set_active(false);
            editor.sections[3].toggle.set_active(true);
            editor.recurrence.frequency.set_selected(2);
            window.set_default_size(900, 850);
            drain();
            screenshot("repeat");
            editor.sections[3].toggle.set_active(false);
            editor.recurrence.frequency.set_selected(0);
            window.set_default_size(900, 700);
            assert!(editor.command().is_err());
            editor.summary.set_text("New event");
            editor.location.set_text("Off");
            editor.description.buffer().set_text("Some details");
            let request = editor.command().unwrap();
            assert_eq!(request.summary, "New event");
            assert_eq!(request.description, "Some details");
            assert_eq!(request.start.as_naive_date().to_string(), "2026-10-15");
            assert!(request.personal_alarms.is_none());
            editor.all_day.set_active(true);
            let request = editor.command().unwrap();
            assert!(
                matches!(request.end, CalDate::Date(date, CalDateType::Exclusive) if date.to_string() == "2026-10-16")
            );
            editor.recurrence.frequency.set_selected(3);
            editor.recurrence.weekdays[3].set_active(true);
            editor.recurrence.end.set_selected(1);
            editor.recurrence.count.set_value(5.0);
            editor.personal_override.set_active(true);
            editor.personal_alarm.trigger.set_selected(1);
            let request = editor.command().unwrap();
            assert_eq!(
                request.recurrence.as_ref().unwrap().frequency(),
                CalRRuleFreq::Weekly
            );
            assert_eq!(request.recurrence.as_ref().unwrap().count(), Some(5));
            assert_eq!(request.personal_alarms.unwrap().len(), 1);
            editor.alarm_mode.set_selected(1);
            assert!(
                editor.previews[1]
                    .text()
                    .starts_with(editor.options.locale.translate("Personal"))
            );
            editor.calendar.set_selected(1);
            assert_eq!(editor.alarm_mode.selected(), 0);
            assert!(editor.command().unwrap().personal_alarms.is_none());
            editor.calendar.set_selected(0);
            editor.set_saving(true);
            assert!(!editor.save.is_sensitive());
            editor.set_saving(false);
            editor.show_error("test error");
            assert_eq!(editor.summary.text(), "New event");
            assert!(editor.save.is_sensitive());
            assert!(editor.error.is_visible());
            editor.new_attendee.set_text("Guest <guest@example.org>");
            assert!(editor.command().is_err());
            // Enter accepts the pending attendee, clears the entry, and updates the summary.
            editor.new_attendee.emit_activate();
            assert!(editor.new_attendee.text().is_empty());
            editor.new_attendee.emit_activate();
            assert_eq!(editor.attendees.borrow().len(), 1);
            assert_eq!(
                editor.command().unwrap().attendees[0].pretty_name(),
                "Guest <guest@example.org>"
            );
            assert_eq!(editor.previews[4].text(), "Guest");
            editor.attendees.borrow()[0].role.set_active(true);
            assert_eq!(
                editor.command().unwrap().attendees[0].role(),
                Some(eventix_ical::objects::CalRole::Optional)
            );
            let attendee_row = editor.attendees.borrow()[0].widget.clone();
            attendee_row
                .last_child()
                .unwrap()
                .downcast::<gtk::Button>()
                .unwrap()
                .emit_clicked();
            assert!(editor.command().unwrap().attendees.is_empty());
            assert_eq!(
                editor.previews[4].text(),
                editor.options.locale.translate("None")
            );
            editor.recurrence.frequency.set_selected(5);
            editor.recurrence.pattern.set_selected(2);
            editor.recurrence.nth.set_selected(3);
            editor.recurrence.month.set_selected(9);
            let request = editor.command().unwrap();
            let recurrence = request.recurrence.unwrap();
            assert_eq!(recurrence.by_month().unwrap(), &vec![10]);
            assert_eq!(
                recurrence.by_day().unwrap()[0].nth(),
                Some((1, eventix_ical::objects::CalRRuleSide::End))
            );
            editor.sections[3].toggle.set_active(true);
            window.set_default_size(900, 950);
            drain();
            screenshot("yearly");
            editor.sections[3].toggle.set_active(false);
            editor.all_day.set_active(false);
            editor.recurrence.end.set_selected(2);
            editor.recurrence.until.entry.set_text("2026-10-16");
            editor.personal_alarm.trigger.set_selected(2);
            assert!(editor.command().is_ok());
            // Expanded sections remain scrollable inside a smaller application window.
            for section in &editor.sections {
                section.toggle.set_active(true);
                assert_eq!(
                    editor
                        .sections
                        .iter()
                        .filter(|s| s.toggle.is_active())
                        .count(),
                    1
                );
            }
            window.set_default_size(800, 500);
            drain();
            assert!(panel.height() <= overlay.height());
            assert!(panel.width() <= overlay.width());
            editor.end.entry.set_text("2026-10-14");
            editor.new_attendee.set_text("");
            assert!(editor.command().is_err());
            window.close();
        }
    }
}
