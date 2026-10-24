// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use anyhow::bail;
use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use eventix_ical::objects::{
    CalAction, CalAlarm, CalAttendee, CalCompType, CalDate, CalRelated, CalRole, CalTrigger,
    DateContext,
};
use eventix_locale::Locale;
use eventix_state::actions::items::{CreateItem, UpdateItem};
use eventix_view::editor::EditorOptions;
use gtk::{gdk, glib, prelude::*};

mod completion;
mod existing;
mod feedback;
mod layout;
mod recurrence;
mod task_status;
mod validation;
use layout::{FormSection, bind_choice, body_label, calendar_factory, row, segments};
use recurrence::RecurrenceInput;

/// Owns the draft independently of calendar refreshes and the details popup.
pub(crate) struct ItemEditor {
    pub layer: gtk::Box,
    panel: gtk::Box,
    scroll: gtk::ScrolledWindow,
    pub save: gtk::Button,
    pub cancel: gtk::Button,
    pub id: u64,
    pub saving: Cell<bool>,
    pub existing: RefCell<Option<UpdateItem>>,
    initial: RefCell<Option<Box<CreateItem>>>,
    title: gtk::Label,
    form: gtk::Box,
    error: gtk::Label,
    options: EditorOptions,
    calendar: gtk::DropDown,
    summary: gtk::Entry,
    location: gtk::Entry,
    description: gtk::TextView,
    task_status: Option<task_status::TaskStatusInput>,
    start: DateInput,
    end: DateInput,
    start_time: gtk::Entry,
    end_time: gtk::Entry,
    all_day: gtk::CheckButton,
    has_start: gtk::CheckButton,
    has_due: gtk::CheckButton,
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
    feedback: Vec<feedback::FieldFeedback>,
    _completions: Vec<completion::Completion>,
}

impl ItemEditor {
    pub fn new(id: u64, options: EditorOptions) -> Rc<Self> {
        Self::new_at(id, options, false)
    }

    pub(crate) fn new_at(id: u64, options: EditorOptions, all_day: bool) -> Rc<Self> {
        let mut editor = Self::build(id, options);
        if all_day {
            editor
                .end
                .entry
                .set_text(&editor.options.start.date().to_string());
            editor.all_day.set_active(true);
        }

        editor.install_feedback();

        let editor = Rc::new(editor);
        editor.connect_timezone();
        editor.update_previews();
        editor.connect_previews();
        editor.connect_task_dates();

        editor
    }

    fn build(id: u64, options: EditorOptions) -> Self {
        let locale = options.locale.as_ref();
        let shell = EditorShell::new(&options);
        let form = shell.form.clone();

        let calendar = Self::create_calendar(&options);
        row(&form, locale, "Calendar", &calendar);

        let summary = gtk::Entry::new();
        row(&form, locale, "Summary", &summary);

        let location = gtk::Entry::new();
        row(&form, locale, "Location", &location);
        let location_completion = completion::Completion::new(&location, options.locations.clone());

        let task_status = (options.kind == CalCompType::Todo)
            .then(|| task_status::TaskStatusInput::new(&form, &options));

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
            panel: shell.panel,
            scroll: shell.scroll,
            save: shell.save,
            cancel: shell.cancel,
            id,
            saving: Cell::new(false),
            existing: RefCell::new(None),
            initial: RefCell::new(None),
            title: shell.title,
            form,
            error: shell.error,
            options,
            calendar,
            summary,
            location,
            description,
            task_status,
            start: dates.start,
            end: dates.end,
            start_time: dates.start_time,
            end_time: dates.end_time,
            all_day: dates.all_day,
            has_start: dates.has_start,
            has_due: dates.has_due,
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
            feedback: Vec::new(),
            _completions: vec![location_completion, attendees.completion],
        }
    }

    fn connect_timezone(self: &Rc<Self>) {
        let previous = Cell::new(self.timezone.selected());
        let weak = Rc::downgrade(self);
        self.timezone.connect_selected_notify(move |timezone| {
            let old = previous.replace(timezone.selected());
            let Some(editor) = weak.upgrade() else {
                return;
            };

            if editor.all_day.is_active() {
                return;
            }

            let (Some(from), Some(to)) = (
                chrono_tz::TZ_VARIANTS.get(old as usize),
                chrono_tz::TZ_VARIANTS.get(timezone.selected() as usize),
            ) else {
                return;
            };

            let Ok((start, end)) = editor.timed_dates(from) else {
                return;
            };

            let context = DateContext::system();
            if let Some(start) = start {
                let start = context.date(&start).start_in(to);
                editor
                    .start
                    .entry
                    .set_text(&start.format("%Y-%m-%d").to_string());
                editor
                    .start_time
                    .set_text(&start.format("%H:%M").to_string());
            }

            if let Some(end) = end {
                let end = context.date(&end).start_in(to);
                editor
                    .end
                    .entry
                    .set_text(&end.format("%Y-%m-%d").to_string());
                editor.end_time.set_text(&end.format("%H:%M").to_string());
            }
        });
    }

    fn connect_previews(self: &Rc<Self>) {
        watch_fields(self.form.upcast_ref(), self);

        // Selection models for segmented controls and hidden stack pages are not widget children.
        for selection in [
            &self.recurrence.frequency,
            &self.recurrence.end,
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

        let weak = Rc::downgrade(self);
        self.personal_override.connect_toggled(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.update_previews();
            }
        });
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
        if let Some(index) = options
            .calendars
            .iter()
            .position(|cal| Some(&cal.id) == options.last_calendar.as_ref())
        {
            calendar.set_selected(index as u32);
        }

        let factory = calendar_factory(&options.calendars, &calendar.model().unwrap());
        calendar.set_factory(Some(&factory));
        calendar.set_list_factory(Some(&factory));

        calendar
    }

    fn create_description(form: &gtk::Box, locale: &dyn Locale) -> (gtk::TextView, FormSection) {
        let builder = gtk::Builder::from_resource(resource_path!("view/editor/description.ui"));
        let description = builder
            .object("description")
            .expect("description input is defined in GTK resources");
        let scroll: gtk::ScrolledWindow = builder
            .object("scroll")
            .expect("description scroller is defined in GTK resources");
        let section = FormSection::new(form, locale, "Description", &scroll, None);

        (description, section)
    }

    pub fn focus(&self) {
        self.summary.grab_focus();
    }

    pub fn kind(&self) -> CalCompType {
        self.options.kind
    }

    fn connect_task_dates(self: &Rc<Self>) {
        if self.kind() != CalCompType::Todo {
            return;
        }

        self.update_task_reminders();
        for toggle in [&self.has_start, &self.has_due] {
            let weak = Rc::downgrade(self);
            toggle.connect_toggled(move |_| {
                if let Some(editor) = weak.upgrade() {
                    editor.update_task_reminders();
                }
            });
        }
    }

    fn update_task_reminders(&self) {
        for alarm in [&self.alarm, &self.personal_alarm] {
            alarm.set_dates(
                self.has_start.is_active(),
                self.has_due.is_active(),
                self.options.locale.as_ref(),
            );
        }
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
        if self.kind() == CalCompType::Todo {
            let date = |date: &DateInput, time: &gtk::Entry| {
                if self.all_day.is_active() {
                    date.entry.text().to_string()
                } else {
                    format!("{} {}", date.entry.text(), time.text())
                }
            };
            let mut dates = Vec::new();
            if self.has_start.is_active() {
                dates.push(format!(
                    "{}: {}",
                    locale.translate("Start"),
                    date(&self.start, &self.start_time)
                ));
            }

            if self.has_due.is_active() {
                dates.push(format!(
                    "{}: {}",
                    locale.translate("Due"),
                    date(&self.end, &self.end_time)
                ));
            }
            self.previews[0].set_text(&if dates.is_empty() {
                locale.translate("Unplanned").to_string()
            } else {
                dates.join(" – ")
            });
            return;
        }

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

        for (index, section) in self.sections.iter().enumerate() {
            let occurrence_recurrence = index == 3
                && self.existing.borrow().as_ref().is_some_and(|edit| {
                    edit.scope == eventix_state::actions::items::EditScope::Occurrence
                });
            section
                .toggle
                .set_sensitive(!saving && !occurrence_recurrence);
        }
    }

    pub fn show_error(&self, message: &str) {
        self.error.set_text(message);
        self.error.set_visible(true);
    }

    fn timed_dates(
        &self,
        timezone: &chrono_tz::Tz,
    ) -> anyhow::Result<(Option<CalDate>, Option<CalDate>)> {
        let locale = self.options.locale.as_ref();
        let start_date = self
            .has_start
            .is_active()
            .then(|| self.start.date(locale))
            .transpose()?;
        let end_date = self
            .has_due
            .is_active()
            .then(|| self.end.date(locale))
            .transpose()?;
        let (start, end) = (
            start_date
                .map(|date| {
                    parse_time(&self.start_time, locale)
                        .map(|time| CalDate::new_datetime(date, Some(time), timezone.name()))
                })
                .transpose()?,
            end_date
                .map(|date| {
                    parse_time(&self.end_time, locale)
                        .map(|time| CalDate::new_datetime(date, Some(time), timezone.name()))
                })
                .transpose()?,
        );
        if let (Some(start), Some(end)) = (&start, &end)
            && start > end
        {
            bail!(locale.translate("error.end_before_start").to_string());
        }

        Ok((start, end))
    }
}

struct EditorShell {
    title: gtk::Label,
    layer: gtk::Box,
    panel: gtk::Box,
    scroll: gtk::ScrolledWindow,
    form: gtk::Box,
    save: gtk::Button,
    cancel: gtk::Button,
    error: gtk::Label,
}

impl EditorShell {
    fn new(options: &EditorOptions) -> Self {
        let locale = options.locale.as_ref();
        let builder = gtk::Builder::from_resource(resource_path!("view/editor/shell.ui"));
        let shell = Self {
            title: builder.object("title").expect("editor title"),
            layer: builder.object("layer").expect("editor backdrop"),
            panel: builder.object("panel").expect("editor panel"),
            scroll: builder.object("scroll").expect("editor scroller"),
            form: builder.object("form").expect("editor form"),
            save: builder.object("save").expect("editor save button"),
            cancel: builder.object("cancel").expect("editor cancel button"),
            error: builder.object("error").expect("editor error label"),
        };
        // Editor and details widths share the same popup-placement contract.
        shell.panel.set_size_request(super::ITEM_PANEL_WIDTH, -1);
        shell
            .title
            .set_text(locale.translate(if options.kind == CalCompType::Todo {
                "New Task"
            } else {
                "New Event"
            }));
        for (id, text) in [("save_label", "Save"), ("cancel_label", "Cancel")] {
            builder
                .object::<gtk::Label>(id)
                .expect("editor action label")
                .set_text(locale.translate(text));
        }

        if options.calendars.is_empty() {
            shell
                .error
                .set_text(locale.translate("error.create_calendar_first"));
            shell.error.set_visible(true);
            shell.save.set_sensitive(false);
        }

        shell
    }
}

struct DateSection {
    section: FormSection,
    start: DateInput,
    end: DateInput,
    start_time: gtk::Entry,
    end_time: gtk::Entry,
    all_day: gtk::CheckButton,
    has_start: gtk::CheckButton,
    has_due: gtk::CheckButton,
    timezone: gtk::DropDown,
}

impl DateSection {
    fn new(form: &gtk::Box, options: &EditorOptions) -> Self {
        let locale = options.locale.as_ref();
        let start = DateInput::new(locale, options.start.date());
        let end_value = options.start + Duration::hours(1);
        let end = DateInput::new(locale, end_value.date());
        Self::connect_dates(&start, &end);

        let start_time = time_entry(options.start.time());
        let end_time = time_entry(end_value.time());
        let all_day = gtk::CheckButton::new();
        all_day.set_halign(gtk::Align::Start);
        let timezone = Self::create_timezone(locale);

        let dates = gtk::Grid::builder()
            .column_spacing(6)
            .row_spacing(8)
            .build();
        dates.add_css_class("ev_editor_dates");
        let start_fields = Self::date_fields(&start, &start_time);
        let end_fields = Self::date_fields(&end, &end_time);
        let has_start = gtk::CheckButton::with_label(locale.translate("Start"));
        let has_due = gtk::CheckButton::with_label(locale.translate("Due"));

        if options.kind == CalCompType::Todo {
            dates.attach(&has_start, 0, 0, 1, 1);
            dates.attach(&has_due, 2, 0, 1, 1);
            start_fields.set_sensitive(false);
            end_fields.set_sensitive(false);

            let fields = start_fields.clone();
            has_start.connect_toggled(move |button| fields.set_sensitive(button.is_active()));

            let fields = end_fields.clone();
            has_due.connect_toggled(move |button| fields.set_sensitive(button.is_active()));
        } else {
            has_start.set_active(true);
            has_due.set_active(true);
            Self::attach_label(&dates, locale, "Start", 0, 0, start.entry.upcast_ref());
            Self::attach_label(&dates, locale, "End", 2, 0, end.entry.upcast_ref());
        }

        Self::attach_label(&dates, locale, "All day", 0, 1, all_day.upcast_ref());
        Self::attach_label(&dates, locale, "Timezone", 2, 1, timezone.upcast_ref());
        dates.attach(&start_fields, 1, 0, 1, 1);
        dates.attach(&end_fields, 3, 0, 1, 1);
        dates.attach(&all_day, 1, 1, 1, 1);

        let timezone_field = gtk::Box::new(gtk::Orientation::Vertical, 0);
        timezone_field.set_hexpand(true);
        timezone_field.append(&timezone);
        dates.attach(&timezone_field, 3, 1, 1, 1);
        Self::connect_all_day(&all_day, &start_time, &end_time, &timezone);
        all_day.set_active(options.kind == CalCompType::Todo);

        let section = FormSection::new(form, locale, "Date", &dates, None);

        Self {
            section,
            start,
            end,
            start_time,
            end_time,
            all_day,
            has_start,
            has_due,
            timezone,
        }
    }

    fn connect_dates(start: &DateInput, end: &DateInput) {
        for (source, target, is_start) in [(start, end, true), (end, start, false)] {
            let target = target.entry.downgrade();
            let input = source.entry.downgrade();
            let synchronize = Rc::new(move || {
                let (Some(source), Some(target)) = (input.upgrade(), target.upgrade()) else {
                    return;
                };

                let (Ok(date), Ok(other)) = (
                    NaiveDate::parse_from_str(source.text().as_str(), "%Y-%m-%d"),
                    NaiveDate::parse_from_str(target.text().as_str(), "%Y-%m-%d"),
                ) else {
                    return;
                };

                if (is_start && date > other) || (!is_start && date < other) {
                    target.set_text(&date.to_string());
                }
            });

            let on_blur = synchronize.clone();
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(move |_| on_blur());
            source.entry.add_controller(focus);
            source.connect_date_selected(move || synchronize());
        }
    }

    fn attach_label(
        grid: &gtk::Grid,
        locale: &dyn Locale,
        name: &str,
        column: i32,
        row: i32,
        field: &gtk::Widget,
    ) {
        let label = body_label(locale, name);
        label.set_text(&format!("{}:", locale.translate(name)));
        label.set_mnemonic_widget(Some(field));
        field.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
        grid.attach(&label, column, row, 1, 1);
    }

    fn date_fields(date: &DateInput, time: &gtk::Entry) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row.set_hexpand(true);
        date.widget.set_valign(gtk::Align::Center);
        date.entry.set_width_chars(10);
        date.entry.set_max_width_chars(10);

        time.set_hexpand(true);
        time.set_valign(gtk::Align::Center);
        time.set_max_width_chars(5);
        row.append(&date.widget);
        row.append(time);

        row
    }

    fn create_timezone(locale: &dyn Locale) -> gtk::DropDown {
        let zones: Vec<_> = chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name()).collect();
        let timezone = gtk::DropDown::from_strings(&zones);
        timezone.set_hexpand(true);
        timezone.set_factory(Some(&Self::timezone_factory(true)));
        timezone.set_list_factory(Some(&Self::timezone_factory(false)));
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

    fn timezone_factory(compact: bool) -> gtk::SignalListItemFactory {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };

            let label = gtk::Label::builder().xalign(0.0).build();
            if compact {
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.set_width_chars(1);
                label.set_max_width_chars(18);
            }
            item.set_child(Some(&label));
        });

        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };

            if let (Some(label), Some(zone)) = (
                item.child().and_downcast::<gtk::Label>(),
                item.item().and_downcast::<gtk::StringObject>(),
            ) {
                label.set_text(&zone.string());
                label.set_tooltip_text(Some(&zone.string()));
            }
        });

        factory
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
    let builder = gtk::Builder::from_resource(resource_path!("view/editor/time_entry.ui"));
    let entry: gtk::Entry = builder.object("time").expect("editor time entry");
    entry.set_text(&time.format("%H:%M").to_string());

    entry
}

fn parse_time(entry: &gtk::Entry, locale: &dyn Locale) -> anyhow::Result<NaiveTime> {
    NaiveTime::parse_from_str(entry.text().as_str(), "%H:%M")
        .map_err(|_| anyhow::anyhow!(locale.translate("error.valid_date_time").to_string()))
}

struct DateInput {
    widget: gtk::Box,
    entry: gtk::Entry,
    calendar: gtk::Calendar,
    syncing: Rc<Cell<bool>>,
}

impl DateInput {
    fn new(locale: &dyn Locale, date: NaiveDate) -> Self {
        let builder = gtk::Builder::from_resource(resource_path!("view/editor/date_input.ui"));
        let widget = builder.object("date_input").expect("editor date input");
        let entry: gtk::Entry = builder.object("entry").expect("editor date entry");
        entry.set_text(&date.to_string());
        let calendar = builder.object("calendar").expect("editor date calendar");
        let picker: gtk::MenuButton = builder.object("picker").expect("editor date picker");
        picker.set_tooltip_text(Some(locale.translate("Choose date")));
        let syncing = Rc::new(Cell::new(false));
        Self::connect_picker_open(&entry, &picker, &calendar, syncing.clone());
        Self::connect_day_selected(&entry, &picker, &calendar, syncing.clone());

        Self {
            widget,
            entry,
            calendar,
            syncing,
        }
    }

    fn connect_date_selected(&self, callback: impl Fn() + 'static) {
        let syncing = self.syncing.clone();
        self.calendar.connect_day_selected(move |_| {
            if !syncing.get() {
                callback();
            }
        });
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
    relations: RefCell<Vec<u32>>,
    relative_row: gtk::Widget,
    date: DateInput,
    time: gtk::Entry,
}

impl AlarmInput {
    fn new(locale: &dyn Locale, date: NaiveDate, time: NaiveTime) -> Self {
        let builder = gtk::Builder::from_resource(resource_path!("view/editor/alarm_input.ui"));
        let widget = builder.object("alarm_input").expect("editor alarm input");
        let trigger = dropdown(locale, &["None", "Relative", "Absolute"]);
        for (index, (id, label)) in [
            ("none", "None"),
            ("relative_choice", "Relative"),
            ("absolute_choice", "Absolute"),
        ]
        .into_iter()
        .enumerate()
        {
            let button: gtk::CheckButton = builder.object(id).expect("editor alarm choice");
            button.set_label(Some(locale.translate(label)));
            bind_choice(&button, &trigger, index as u32);
        }

        let duration = builder.object("duration").expect("editor alarm duration");
        let unit: gtk::DropDown = builder.object("unit").expect("editor alarm unit");
        let names: Vec<_> = ["Minutes", "Hours", "Days"]
            .iter()
            .map(|name| locale.translate(name))
            .collect();
        unit.set_model(Some(&gtk::StringList::new(&names)));
        let relation: gtk::DropDown = builder.object("relation").expect("editor alarm relation");
        let names: Vec<_> = ["BeforeStart", "AfterStart", "BeforeEnd", "AfterEnd"]
            .iter()
            .map(|name| locale.translate(name))
            .collect();
        relation.set_model(Some(&gtk::StringList::new(&names)));

        let relative: gtk::Box = builder.object("relative").expect("editor relative alarm");
        let absolute: gtk::Box = builder.object("absolute").expect("editor absolute alarm");
        let date = DateInput::new(locale, date);
        date.widget.set_hexpand(false);
        let time = time_entry(time);
        absolute.append(&date.widget);
        absolute.append(&time);
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

        Ok(self.alarms(trigger, locale))
    }

    fn alarms(&self, trigger: CalTrigger, locale: &dyn Locale) -> Vec<CalAlarm> {
        let mut alarm = CalAlarm::new(CalAction::Display, trigger);
        alarm.set_description(Some(locale.translate("Reminder").to_string()));

        vec![alarm]
    }

    fn relative_trigger(&self) -> CalTrigger {
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

    fn set_dates(&self, start: bool, due: bool, locale: &dyn Locale) {
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
    original: Option<CalAttendee>,
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
            original: None,
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
        let mut attendee = self
            .original
            .clone()
            .unwrap_or_else(|| CalAttendee::new(format!("mailto:{address}")));
        if let Some(name) = name {
            attendee.set_common_name(name.to_string());
        }

        if self.original.as_ref().is_none_or(|original| {
            (original.role() == Some(CalRole::Optional)) != self.role.is_active()
        }) {
            attendee.set_role(if self.role.is_active() {
                CalRole::Optional
            } else {
                CalRole::Required
            });
        }

        attendee
    }
}

/// Gives nested GTK popovers first refusal on Escape.
pub(crate) fn has_nested_popup(window: &gtk::Window) -> bool {
    let mut focused = gtk::prelude::GtkWindowExt::focus(window);
    while let Some(widget) = focused {
        let mut child = widget.first_child();
        while let Some(current) = child {
            if current.is::<gtk::Popover>()
                && !current.is::<super::details::EventDetails>()
                && current.is_visible()
            {
                return true;
            }

            child = current.next_sibling();
        }

        if widget.is::<gtk::Popover>()
            && !widget.is::<super::details::EventDetails>()
            && widget.is_visible()
        {
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
    use eventix_ical::objects::{CalDateType, CalRRuleFreq};
    use eventix_state::actions::Action;
    use eventix_view::editor::{EditorCalendar, EditorOptions};
    use gtk::prelude::*;

    use super::{AlarmInput, CalDate, DateInput, DateSection, ItemEditor};

    fn assert_slot_creation_defaults() {
        for (start, all_day, expected_end) in [
            ("2026-10-22T10:00:00", false, "2026-10-22T11:00:00"),
            ("2026-10-22T23:00:00", false, "2026-10-23T00:00:00"),
            ("2026-10-22T00:00:00", true, "2026-10-23T00:00:00"),
        ] {
            let options = EditorOptions {
                kind: eventix_ical::objects::CalCompType::Event,
                calendars: vec![EditorCalendar {
                    id: "test".into(),
                    name: "Test".into(),
                    background: "#27c281".into(),
                    personal: false,
                }],
                last_calendar: None,
                locations: vec![],
                contacts: vec![],
                start: start.parse().unwrap(),
                locale: eventix_locale::default(),
            };
            let timezone = *options.locale.timezone();
            let editor = ItemEditor::new_at(1, options, all_day);
            editor.summary.set_text("Slot creation");
            let command = editor.command().unwrap();
            let context = eventix_ical::objects::DateContext::system();
            assert_eq!(editor.all_day.is_active(), all_day);
            for (date, expected) in [
                (command.start.as_ref().unwrap(), start),
                (command.end.as_ref().unwrap(), expected_end),
            ] {
                assert_eq!(
                    context.date(date).start_in(&timezone).naive_local(),
                    expected.parse::<chrono::NaiveDateTime>().unwrap(),
                );
                assert_eq!(matches!(date, CalDate::Date(..)), all_day);
            }
        }
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn alarm_choices_synchronize_modes_and_preserve_trigger_values() {
        use eventix_ical::objects::{CalRelated, CalTrigger};

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        for language in [
            eventix_locale::LocaleType::English,
            eventix_locale::LocaleType::German,
        ] {
            let locale = eventix_locale::new_with_timezone(&xdg, language, chrono_tz::UTC).unwrap();
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
            let absolute = alarm
                .widget
                .last_child()
                .unwrap()
                .first_child()
                .and_downcast::<gtk::CheckButton>()
                .unwrap();
            let relative_controls = alarm.relative_row.last_child().unwrap();
            let absolute_controls = alarm.widget.last_child().unwrap().last_child().unwrap();
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
            assert!(matches!(
                alarm.value(&chrono_tz::UTC, locale.as_ref()).unwrap()[0].trigger(),
                CalTrigger::Relative { duration, related: CalRelated::Start }
                    if *duration == chrono::Duration::minutes(-15).into()
            ));

            absolute.set_active(true);
            assert_eq!(alarm.trigger.selected(), 2);
            assert!(!relative.is_active() && !relative_controls.is_sensitive());
            assert!(absolute_controls.is_sensitive());
            assert!(matches!(
                alarm.value(&chrono_tz::UTC, locale.as_ref()).unwrap()[0].trigger(),
                CalTrigger::Absolute(date)
                    if date.to_string() == "TU2026-10-15T10:30:00"
            ));

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

    #[test]
    #[ignore = "requires a GTK display"]
    fn task_editor_supports_optional_dates_status_and_date_aware_reminders() {
        use std::{cell::Cell, rc::Rc, sync::Arc};

        use eventix_ical::{
            col::CalDir,
            objects::{CalCompType, CalRelated, CalTodoStatus, CalTrigger},
        };
        use eventix_state::{
            CalendarSettings, CollectionSettings, State, SyncerType,
            actions::items::{EditScope, UpdateItem},
        };

        use gtk::subclass::prelude::ObjectSubclassIsExt;

        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        assert_slot_creation_defaults();
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        state.store_mut().add(CalDir::new_empty(
            Arc::new("tasks".into()),
            tmp.path().into(),
            "Tasks".into(),
            false,
        ));
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: tmp.path().display().to_string(),
        });
        let mut calendar = CalendarSettings::default();
        calendar.set_enabled(true);
        calendar.set_types(vec![CalCompType::Todo]);
        collection
            .all_calendars_mut()
            .insert("tasks".into(), calendar);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        assert!(EditorOptions::load(&state).unwrap().calendars.is_empty());
        let options = || EditorOptions::load_for(&state, CalCompType::Todo).unwrap();
        let editor = ItemEditor::new(1, options());
        assert_eq!(editor.title.text(), "New Task");
        assert!(editor.all_day.is_active());
        assert!(!editor.has_start.is_active() && !editor.has_due.is_active());
        assert!(!editor.alarm.relative_row.is_visible());
        editor.summary.set_text("Task");
        editor.start.entry.set_text("ignored while disabled");
        editor.end.entry.set_text("ignored while disabled");
        let fields = editor.command().unwrap();
        assert!(fields.start.is_none() && fields.end.is_none());
        assert_eq!(
            fields.task_status.as_ref().unwrap().status,
            CalTodoStatus::NeedsAction
        );
        let item = fields.execute(&mut state).unwrap();

        let options = || EditorOptions::load_for(&state, CalCompType::Todo).unwrap();
        let editor = ItemEditor::edit(
            2,
            options(),
            UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap(),
        );
        assert_eq!(editor.title.text(), "Edit Task");
        assert!(!editor.has_start.is_active() && !editor.has_due.is_active());
        editor.has_due.set_active(true);
        editor.end.entry.set_text("2026-10-15");
        assert!(editor.alarm.relative_row.is_visible());
        editor.alarm.trigger.set_selected(1);
        let status = editor.task_status.as_ref().unwrap();
        status.status.set_selected(2);
        let fields = editor.command().unwrap();
        assert!(fields.start.is_none());
        assert!(
            matches!(fields.end, Some(CalDate::Date(date, CalDateType::Inclusive)) if date.to_string() == "2026-10-15")
        );
        assert!(matches!(
            fields.alarms[0].trigger(),
            CalTrigger::Relative {
                related: CalRelated::End,
                ..
            }
        ));
        assert_eq!(
            fields.task_status.as_ref().unwrap().status,
            CalTodoStatus::InProcess
        );
        let mut edit = editor.existing.borrow().clone().unwrap();
        edit.fields = *fields;
        Box::new(edit).execute(&mut state).unwrap();

        let editor = ItemEditor::edit(
            3,
            EditorOptions::load_for(&state, CalCompType::Todo).unwrap(),
            UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap(),
        );
        assert!(editor.has_due.is_active());
        assert_eq!(editor.end.entry.text(), "2026-10-15");
        editor.recurrence.frequency.set_selected(2);
        assert!(
            editor
                .command()
                .unwrap_err()
                .iter()
                .any(|issue| issue.field == super::validation::Field::StartDate)
        );
        editor.recurrence.frequency.set_selected(0);
        let status = editor.task_status.as_ref().unwrap();
        status.status.set_selected(1);
        status.has_completed.set_active(true);
        status.completed.entry.set_text("invalid");
        assert!(
            editor
                .command()
                .unwrap_err()
                .iter()
                .any(|issue| issue.field == super::validation::Field::CompletedDate)
        );
        status.completed.entry.set_text("2026-10-16");
        let fields = editor.command().unwrap();
        assert_eq!(fields.task_status.as_ref().unwrap().percent, Some(100));
        assert_eq!(
            fields
                .task_status
                .as_ref()
                .unwrap()
                .completed
                .as_ref()
                .unwrap()
                .to_string(),
            "TU2026-10-16T00:00:00"
        );
        editor.has_due.set_active(false);
        assert_eq!(editor.alarm.trigger.selected(), 0);
        assert!(editor.command().unwrap().alarms.is_empty());

        let parent: crate::view::page::CalendarPage = gtk::glib::Object::new();
        let window = gtk::Window::builder()
            .child(&parent)
            .default_width(1000)
            .default_height(900)
            .build();
        let anchor = gtk::Button::with_label("Task");
        parent.append(&anchor);
        window.present();
        let drain = || {
            for _ in 0..30 {
                while gtk::glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        drain();
        let task = eventix_view::sidebar::SidebarRow::load(&state, &item).unwrap();
        let controller = parent.test_details();
        let clicked = Rc::new(Cell::new(false));
        let flag = clicked.clone();
        let identity = item.clone();
        controller.connect_edit(move |item, scope| {
            assert_eq!(item, identity);
            assert_eq!(scope, EditScope::Series);
            flag.set(true);
        });
        let popup = crate::view::details::EventDetails::new_sidebar(&task, state.locale().as_ref());
        controller.show_details(anchor.upcast_ref(), anchor.upcast_ref(), popup.clone());
        drain();
        popup.imp().edits.borrow()[0].0.emit_clicked();
        assert!(clicked.get());
        let editor = parent.test_existing_editor(
            EditorOptions::load_for(&state, CalCompType::Todo).unwrap(),
            UpdateItem::load(&state, item.clone(), EditScope::Series).unwrap(),
        );
        assert!(parent.editor_open());
        assert_eq!(editor.summary.text(), "Task");
        assert!(editor.has_due.is_active());
        editor.summary.set_text("Edited task");
        let mut edit = editor.existing.borrow().clone().unwrap();
        edit.fields = *editor.command().unwrap();
        Box::new(edit).execute(&mut state).unwrap();
        let saved = eventix_view::sidebar::SidebarRow::load(&state, &item).unwrap();
        assert_eq!(saved.summary, "Edited task");
        parent.test_saved_editor(crate::model::session::SavedItem::Task(Box::new(saved)));
        drain();
        assert!(!parent.editor_open());
        assert_eq!(controller.current(), Some(popup));
        controller.close();
        window.close();
        drain();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn timezone_changes_preserve_event_instants() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        for (from, to, start, end, expected_start, expected_end) in [
            (
                chrono_tz::Europe::Berlin,
                chrono_tz::America::Los_Angeles,
                "2026-10-15T00:30:00",
                "2026-10-17T01:30:00",
                "2026-10-14 15:30",
                "2026-10-16 16:30",
            ),
            (
                chrono_tz::America::Los_Angeles,
                chrono_tz::Asia::Tokyo,
                "2026-10-15T23:30:00",
                "2026-10-16T00:30:00",
                "2026-10-16 15:30",
                "2026-10-16 16:30",
            ),
            (
                chrono_tz::Europe::Berlin,
                chrono_tz::UTC,
                "2026-03-29T01:30:00",
                "2026-03-29T03:30:00",
                "2026-03-29 00:30",
                "2026-03-29 01:30",
            ),
            (
                chrono_tz::UTC,
                chrono_tz::Asia::Kathmandu,
                "2026-01-15T23:30:00",
                "2026-01-15T23:45:00",
                "2026-01-16 05:15",
                "2026-01-16 05:30",
            ),
        ] {
            let editor = ItemEditor::new(
                1,
                EditorOptions {
                    kind: eventix_ical::objects::CalCompType::Event,
                    calendars: vec![EditorCalendar {
                        id: "test".into(),
                        name: "Test".into(),
                        background: "#27c281".into(),
                        personal: false,
                    }],
                    last_calendar: None,
                    locations: vec![],
                    contacts: vec![],
                    start: start.parse().unwrap(),
                    locale: eventix_locale::new_with_timezone(
                        &xdg,
                        eventix_locale::LocaleType::English,
                        from,
                    )
                    .unwrap(),
                },
            );
            editor.summary.set_text("Timezone test");
            let end: NaiveDateTime = end.parse().unwrap();
            editor
                .end
                .entry
                .set_text(&end.format("%Y-%m-%d").to_string());
            editor.end_time.set_text(&end.format("%H:%M").to_string());
            let original = editor.command().unwrap();
            let select = |zone| {
                editor.timezone.set_selected(
                    chrono_tz::TZ_VARIANTS
                        .iter()
                        .position(|tz| *tz == zone)
                        .unwrap() as u32,
                );
            };
            select(to);
            assert_eq!(
                format!("{} {}", editor.start.entry.text(), editor.start_time.text()),
                expected_start,
            );
            assert_eq!(
                format!("{} {}", editor.end.entry.text(), editor.end_time.text()),
                expected_end,
            );
            let converted = editor.command().unwrap();
            let context = eventix_ical::objects::DateContext::system();
            for (before, after) in [
                (&original.start, &converted.start),
                (&original.end, &converted.end),
            ] {
                assert_eq!(
                    context
                        .date(before.as_ref().unwrap())
                        .start_in(&chrono_tz::UTC),
                    context
                        .date(after.as_ref().unwrap())
                        .start_in(&chrono_tz::UTC),
                );
            }
            assert!(editor.previews[0].text().contains(to.name()));
            select(from);
            assert_eq!(editor.command().unwrap().start, original.start);
            assert_eq!(editor.command().unwrap().end, original.end);

            editor.all_day.set_active(true);
            let all_day = editor.command().unwrap();
            select(to);
            assert_eq!(editor.command().unwrap().start, all_day.start);
            assert_eq!(editor.command().unwrap().end, all_day.end);
            editor.all_day.set_active(false);
            select(from);
            editor.start_time.set_text("invalid");
            let dates = (editor.start.entry.text(), editor.end.entry.text());
            select(to);
            assert_eq!((editor.start.entry.text(), editor.end.entry.text()), dates);
            assert_eq!(editor.start_time.text(), "invalid");
        }
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn date_inputs_synchronize_on_focus_loss_and_picker_selection() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let xdg = eventix_state::with_test_xdg(tmp.path(), tmp.path());
        let locale = eventix_locale::new_with_timezone(
            &xdg,
            eventix_locale::LocaleType::English,
            chrono_tz::Europe::Berlin,
        )
        .unwrap();
        let date = "2026-10-15".parse().unwrap();
        let start = DateInput::new(locale.as_ref(), date);
        let end = DateInput::new(locale.as_ref(), date);
        DateSection::connect_dates(&start, &end);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.append(&start.widget);
        content.append(&end.widget);
        let other = gtk::Entry::new();
        content.append(&other);
        let window = gtk::Window::builder().child(&content).build();
        window.present();
        while gtk::glib::MainContext::default().iteration(false) {}

        start.entry.grab_focus();
        start.entry.set_text("2026-10-23");
        assert_eq!(end.entry.text(), "2026-10-15");
        other.grab_focus();
        assert_eq!(end.entry.text(), "2026-10-23");

        end.entry.grab_focus();
        end.entry.set_text("2026-10-10");
        assert_eq!(start.entry.text(), "2026-10-23");
        other.grab_focus();
        assert_eq!(start.entry.text(), "2026-10-10");

        end.entry.grab_focus();
        end.entry.set_text("2026-10-20");
        other.grab_focus();
        assert_eq!(start.entry.text(), "2026-10-10");
        start.entry.grab_focus();
        start.entry.set_text("2026-10-15");
        other.grab_focus();
        assert_eq!(end.entry.text(), "2026-10-20");

        start.entry.grab_focus();
        start.entry.set_text("2026-10-");
        other.grab_focus();
        assert_eq!(end.entry.text(), "2026-10-20");
        end.entry.grab_focus();
        end.entry.set_text("2026-10-09");
        other.grab_focus();
        assert_eq!(start.entry.text(), "2026-10-");
        start.entry.grab_focus();
        start.entry.set_text("2026-10-08");
        other.grab_focus();
        assert_eq!(end.entry.text(), "2026-10-09");

        let selected = gtk::glib::DateTime::from_local(2026, 10, 23, 12, 0, 0.0).unwrap();
        start.calendar.select_day(&selected);
        assert_eq!(start.entry.text(), "2026-10-23");
        assert_eq!(end.entry.text(), "2026-10-23");

        let selected = gtk::glib::DateTime::from_local(2026, 10, 10, 12, 0, 0.0).unwrap();
        end.calendar.select_day(&selected);
        assert_eq!(start.entry.text(), "2026-10-10");
        assert_eq!(end.entry.text(), "2026-10-10");
        window.close();
    }

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
                kind: eventix_ical::objects::CalCompType::Event,
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
                // Section revealers animate for 300 ms; wait before focusing their inputs.
                for _ in 0..40 {
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
                panel.width() >= crate::view::ITEM_PANEL_WIDTH - 2 && panel.height() > 350,
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
            assert_eq!(
                request.start.as_ref().unwrap().as_naive_date().to_string(),
                "2026-10-15"
            );
            assert!(request.personal_alarms.is_none());
            editor.all_day.set_active(true);
            let request = editor.command().unwrap();
            assert!(
                matches!(request.end, Some(CalDate::Date(date, CalDateType::Exclusive)) if date.to_string() == "2026-10-16")
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
