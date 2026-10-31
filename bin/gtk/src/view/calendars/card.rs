// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use eventix_ical::objects::CalCompType;
use eventix_locale::Locale;
use eventix_state::{CalendarAlarmType, CalendarSettings};
use eventix_view::calendars::{Backend, Calendar, Collection};
use gtk::{gdk, prelude::*};

use super::{ActionHandler, action, clear, icon_action, text};
use crate::model::management::{Operation, Request};
use crate::view::{calendar_selector::set_dot_color, editor::AlarmInput};

const ROW_TITLES: [&str; 4] = ["Folder", "Color", "Creation", "Alarms"];

/// A stable calendar card whose draft is independent of refreshed management data.
pub(super) struct CalendarCard {
    pub widget: gtk::Frame,
    model: RefCell<Calendar>,
    locale: RefCell<Arc<dyn Locale + Send + Sync>>,
    backend: Cell<Backend>,
    create_calendar: Cell<bool>,
    header: CardHeader,
    actions: CardActions,
    revealer: gtk::Revealer,
    collection: String,
    on_action: ActionHandler,
    draft: RefCell<Option<CalendarForm>>,
    pending: Cell<bool>,
}

impl CalendarCard {
    pub(super) fn new(
        calendar: &Calendar,
        collection: &Collection,
        locale: Arc<dyn Locale + Send + Sync>,
        toggle_group: &gtk::SizeGroup,
        on_action: &ActionHandler,
    ) -> Rc<Self> {
        let builder = gtk::Builder::from_resource(resource_path!("view/calendars/calendar.ui"));
        let heading: gtk::Box = builder.object("heading").unwrap();
        let actions: gtk::Box = builder.object("actions").unwrap();
        let card = Rc::new(Self {
            widget: builder.object("calendar").unwrap(),
            revealer: builder.object("revealer").unwrap(),
            model: RefCell::new(calendar.clone()),
            locale: RefCell::new(locale.clone()),
            backend: Cell::new(collection.backend),
            create_calendar: Cell::new(collection.create_calendar),
            header: CardHeader::new(&heading, calendar, locale.as_ref(), toggle_group),
            actions: CardActions::new(&actions, calendar.configured, locale.as_ref()),
            collection: collection.id.clone(),
            on_action: on_action.clone(),
            draft: RefCell::default(),
            pending: Cell::new(false),
        });
        card.update(calendar, collection, locale);
        card.connect();
        card
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.header.enabled.connect_state_set(move |_, enabled| {
            if let Some(card) = weak.upgrade() {
                if card.model.borrow().enabled == enabled {
                    return gtk::glib::Propagation::Proceed;
                }
                card.submit(Operation::Enable(enabled));
            }
            gtk::glib::Propagation::Stop
        });
        let weak = Rc::downgrade(self);
        self.actions.save.connect_clicked(move |_| {
            if let Some(card) = weak.upgrade() {
                let result = card.draft.borrow().as_ref().map(|draft| {
                    draft.settings(&card.header.entry.text(), card.locale.borrow().as_ref())
                });
                match result {
                    Some(Ok(settings)) => card.submit(Operation::Save(Box::new(settings))),
                    Some(Err(error)) => card.show_error(&error.to_string()),
                    None => {}
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.actions.folder.connect_clicked(move |_| {
            if let Some(card) = weak.upgrade() {
                card.widget.clipboard().set_text(&card.model.borrow().path);
            }
        });
        for (button, operation) in [
            (&self.actions.reload, Operation::Reload),
            (&self.actions.delete, Operation::Delete),
        ] {
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(card) = weak.upgrade() {
                    card.confirm(operation.clone());
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.actions.edit.connect_clicked(move |_| {
            if let Some(card) = weak.upgrade() {
                card.open();
            }
        });
        let weak = Rc::downgrade(self);
        self.actions.cancel.connect_clicked(move |_| {
            if let Some(card) = weak.upgrade() {
                card.close();
            }
        });
    }

    fn submit(&self, operation: Operation) {
        let callback = self.on_action.borrow();
        let Some(callback) = callback.as_ref() else {
            return;
        };
        if self.pending.replace(true) {
            return;
        }
        self.widget.set_sensitive(false);
        let calendar = self.model.borrow();
        callback(Request {
            collection: self.collection.clone(),
            calendar: calendar.id.clone(),
            folder: calendar.folder.clone(),
            configured: calendar.configured,
            operation,
        });
    }

    pub(super) fn finish(&self, request: &Request, success: bool) {
        if !self.pending.replace(false) {
            return;
        }
        self.widget.set_sensitive(true);
        if success && let Operation::Enable(enabled) = request.operation {
            self.model.borrow_mut().enabled = enabled;
        }
        if success
            && matches!(request.operation, Operation::Save(_))
            && self.revealer.reveals_child()
        {
            self.close();
        }
        self.header.enabled.set_state(self.model.borrow().enabled);
        self.header.enabled.set_active(self.model.borrow().enabled);
    }

    fn show_error(&self, message: &str) {
        let locale = self.locale.borrow();
        let dialog = gtk::AlertDialog::builder()
            .message(locale.translate("Could not apply changes"))
            .detail(message)
            .build();
        dialog.show(self.widget.root().and_downcast::<gtk::Window>().as_ref());
    }

    fn confirm(self: &Rc<Self>, operation: Operation) {
        let calendar = self.model.borrow();
        let warning = match operation {
            Operation::Delete if !calendar.configured => {
                Some("warning.delete_calendar_remote_folder")
            }
            Operation::Delete if self.backend.get() != Backend::FileSystem => {
                Some("warning.delete_calendar")
            }
            Operation::Reload if self.backend.get() != Backend::FileSystem => {
                Some("warning.reload_calendar")
            }
            _ => None,
        };
        let Some(warning) = warning else {
            drop(calendar);
            self.submit(operation);
            return;
        };
        let locale = self.locale.borrow();
        let dialog = gtk::AlertDialog::builder()
            .message(locale.translate("Are you sure?"))
            .detail(locale.translate(warning))
            .buttons(vec![
                locale.translate("Cancel"),
                locale.translate(match operation {
                    Operation::Delete => "Delete",
                    _ => "Reload",
                }),
            ])
            .cancel_button(0)
            .default_button(0)
            .build();
        let weak = Rc::downgrade(self);
        dialog.choose(
            self.widget.root().and_downcast::<gtk::Window>().as_ref(),
            None::<&gtk::gio::Cancellable>,
            move |result| {
                if result == Ok(1)
                    && let Some(card) = weak.upgrade()
                {
                    card.submit(operation);
                }
            },
        );
    }

    pub(super) fn update(
        &self,
        calendar: &Calendar,
        collection: &Collection,
        locale: Arc<dyn Locale + Send + Sync>,
    ) {
        let changed = *self.model.borrow() != *calendar
            || self.backend.get() != collection.backend
            || self.create_calendar.get() != collection.create_calendar
            || self.locale.borrow().translate("Calendars") != locale.translate("Calendars")
            || self.actions.view_actions.first_child().is_none();
        self.model.replace(calendar.clone());
        self.backend.set(collection.backend);
        self.create_calendar.set(collection.create_calendar);
        self.locale.replace(locale.clone());
        self.header.update(calendar, self.revealer.reveals_child());
        self.actions.folder.set_tooltip_text(Some(&calendar.path));
        if !changed {
            return;
        }
        self.header.translate(locale.as_ref());
        self.actions
            .update(calendar.configured, collection, locale.as_ref());
    }

    fn open(&self) {
        if self.revealer.reveals_child() {
            return;
        }
        let calendar = self.model.borrow();
        let locale = self.locale.borrow();
        self.header.entry.set_text(&calendar.name);
        let form = calendar_form(
            &calendar,
            self.backend.get(),
            locale.as_ref(),
            &self.header.dot,
            &self.header.field_labels,
        );
        // Replace discarded drafts only when reopening, never during a closing animation.
        self.revealer.set_child(Some(&form.widget));
        self.draft.replace(Some(form));
        self.set_editing(true);
        self.header.entry.grab_focus();
    }

    fn close(&self) {
        self.set_editing(false);
        set_dot_color(&self.header.dot, color(&self.model.borrow().background));
        self.actions.edit.grab_focus();
    }

    fn set_editing(&self, editing: bool) {
        self.header
            .title
            .set_visible_child_name(if editing { "entry" } else { "name" });
        self.actions.set_editing(editing);
        self.revealer.set_reveal_child(editing);
    }
}

struct CardHeader {
    field_labels: [gtk::Label; 4],
    title: gtk::Stack,
    name: gtk::Label,
    entry: gtk::Entry,
    dot: gtk::DrawingArea,
    enabled: gtk::Switch,
}

impl CardHeader {
    fn new(
        container: &gtk::Box,
        calendar: &Calendar,
        locale: &dyn Locale,
        toggle_group: &gtk::SizeGroup,
    ) -> Self {
        let name = text(&calendar.name);
        name.set_wrap(false);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_width_chars(1);
        name.set_hexpand(true);
        let entry = gtk::Entry::builder().hexpand(true).width_chars(1).build();
        entry.add_css_class("ev_management_name");

        // Reserve the entry's height even when showing the collapsed title.
        let title = gtk::Stack::builder()
            .hexpand(true)
            .hhomogeneous(false)
            .vhomogeneous(true)
            .build();
        title.add_named(&name, Some("name"));
        title.add_named(&entry, Some("entry"));

        let header = Self {
            field_labels: ROW_TITLES.map(|title| form_label(title, locale)),
            title,
            name,
            entry,
            dot: gtk::DrawingArea::builder()
                .width_request(22)
                .height_request(22)
                .valign(gtk::Align::Center)
                .build(),
            enabled: gtk::Switch::builder().valign(gtk::Align::Center).build(),
        };
        header.append_to(container, calendar.configured, toggle_group);
        header
    }

    fn append_to(&self, container: &gtk::Box, configured: bool, toggle_group: &gtk::SizeGroup) {
        let leading = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        let leading_group = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        leading_group.add_widget(&leading);
        // Include translated labels before the first expansion so the header never shifts.
        for label in &self.field_labels {
            leading_group.add_widget(label);
        }

        // Reserve the enablement column even for discovered, unconfigured calendars.
        let toggle_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        toggle_slot.set_valign(gtk::Align::Center);
        toggle_group.add_widget(&toggle_slot);
        if configured {
            toggle_slot.append(&self.enabled);
        }
        leading.append(&toggle_slot);
        leading.append(&self.dot);
        container.append(&leading);
        container.append(&self.title);
    }

    fn update(&self, calendar: &Calendar, editing: bool) {
        self.name.set_text(&calendar.name);
        self.name.set_tooltip_text(Some(&calendar.name));
        self.enabled.set_active(calendar.enabled);
        if !editing {
            set_dot_color(&self.dot, color(&calendar.background));
        }
    }

    fn translate(&self, locale: &dyn Locale) {
        for (label, title) in self.field_labels.iter().zip(ROW_TITLES) {
            label.set_text(locale.translate(title));
        }
        label_control(&self.enabled, "Enabled", locale);
        label_control(&self.entry, "Calendar Name", locale);
    }
}

struct CardActions {
    view_actions: gtk::Box,
    edit_actions: gtk::Box,
    edit: gtk::Button,
    cancel: gtk::Button,
    save: gtk::Button,
    folder: gtk::Button,
    reload: gtk::Button,
    delete: gtk::Button,
}

impl CardActions {
    fn new(container: &gtk::Box, configured: bool, locale: &dyn Locale) -> Self {
        let view_actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let edit_actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        edit_actions.set_visible(false);
        let edit = icon_action(
            if configured {
                "pencil-symbolic"
            } else {
                "list-add-symbolic"
            },
            if configured { "Edit" } else { "Add" },
            locale,
        );
        let cancel = action("window-close-symbolic", "Cancel", locale);
        let save = icon_action("document-save-symbolic", "Save", locale);
        let folder = icon_action("folder-symbolic", "Folder", locale);
        let reload = action("view-refresh-symbolic", "Reload", locale);
        let delete = icon_action("user-trash-symbolic", "Delete", locale);
        edit_actions.append(&cancel);
        edit_actions.append(&save);
        container.append(&view_actions);
        container.append(&edit);
        container.append(&edit_actions);
        container.append(&folder);

        Self {
            view_actions,
            edit_actions,
            edit,
            cancel,
            save,
            folder,
            reload,
            delete,
        }
    }

    fn update(&self, configured: bool, collection: &Collection, locale: &dyn Locale) {
        self.translate(configured, locale);
        clear(&self.view_actions);
        if configured {
            let reload = &self.reload;
            let delete = &self.delete;
            reload.remove_css_class("ev_button_warn");
            delete.remove_css_class("ev_button_warn");
            label_control(reload, "Reload", locale);
            label_control(delete, "Delete", locale);
            if collection.backend != Backend::FileSystem {
                reload.add_css_class("ev_button_warn");
                delete.add_css_class("ev_button_warn");
                reload.set_tooltip_text(Some(locale.translate("warning.reload_calendar")));
                delete.set_tooltip_text(Some(locale.translate("warning.delete_calendar")));
            }
            self.view_actions.append(reload);
            self.view_actions.append(delete);
        } else if collection.create_calendar && collection.backend != Backend::FileSystem {
            let delete = &self.delete;
            delete.add_css_class("ev_button_critical");
            delete.set_tooltip_text(Some(
                locale.translate("warning.delete_calendar_remote_folder"),
            ));
            self.view_actions.append(delete);
        }
    }

    fn translate(&self, configured: bool, locale: &dyn Locale) {
        label_control(&self.edit, if configured { "Edit" } else { "Add" }, locale);
        label_control(&self.save, "Save", locale);
        label_control(&self.cancel, "Cancel", locale);
        // The folder tooltip is the path, not the accessible action label.
        self.folder
            .update_property(&[gtk::accessible::Property::Label(locale.translate("Folder"))]);
        if let Some(content) = self.cancel.child().and_downcast::<gtk::Box>()
            && let Some(label) = content.last_child().and_downcast::<gtk::Label>()
        {
            label.set_text(locale.translate("Cancel"));
        }
    }

    fn set_editing(&self, editing: bool) {
        self.view_actions.set_visible(!editing);
        self.edit.set_visible(!editing);
        self.edit_actions.set_visible(editing);
        self.edit
            .update_state(&[gtk::accessible::State::Expanded(Some(editing))]);
    }
}

fn form_label(title: &str, locale: &dyn Locale) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(locale.translate(title))
        .xalign(0.0)
        .valign(gtk::Align::Center)
        .build();
    label.add_css_class("ev_management_form_label");
    label
}

struct CalendarForm {
    widget: gtk::Box,
    folder: gtk::Entry,
    background: gtk::ColorDialogButton,
    foreground: gtk::ColorDialogButton,
    events: gtk::CheckButton,
    tasks: gtk::CheckButton,
    personal: gtk::ToggleButton,
    alarm: Rc<AlarmInput>,
}

impl CalendarForm {
    fn settings(&self, name: &str, locale: &dyn Locale) -> anyhow::Result<CalendarSettings> {
        let mut settings = CalendarSettings::default();
        settings.set_name(name.to_owned());
        settings.set_folder(self.folder.text().to_string());
        settings.set_bgcolor(hex_color(self.background.rgba()));
        settings.set_fgcolor(hex_color(self.foreground.rgba()));
        settings.set_types(
            [
                (self.events.is_active(), CalCompType::Event),
                (self.tasks.is_active(), CalCompType::Todo),
            ]
            .into_iter()
            .filter_map(|(active, kind)| active.then_some(kind))
            .collect(),
        );
        settings.set_alarms(if self.personal.is_active() {
            CalendarAlarmType::Personal {
                default: self
                    .alarm
                    .value_with_text(
                        locale.timezone(),
                        locale.translate("error.valid_date_time"),
                        locale.translate("Reminder"),
                    )?
                    .into_iter()
                    .next(),
            }
        } else {
            CalendarAlarmType::Calendar
        });
        Ok(settings)
    }
}

fn hex_color(color: gdk::RGBA) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (color.red() * 255.0).round() as u8,
        (color.green() * 255.0).round() as u8,
        (color.blue() * 255.0).round() as u8,
    )
}

fn calendar_form(
    calendar: &Calendar,
    backend: Backend,
    locale: &dyn Locale,
    dot: &gtk::DrawingArea,
    labels: &[gtk::Label; 4],
) -> CalendarForm {
    let form = gtk::Box::new(gtk::Orientation::Vertical, 6);
    form.add_css_class("ev_management_form");
    let folder = gtk::Entry::builder()
        .text(&calendar.folder)
        .editable(backend == Backend::FileSystem)
        .hexpand(true)
        .width_chars(1)
        .build();
    append_row(&form, &labels[0], "Folder", &folder, locale);

    let (colors, background, foreground) = color_controls(calendar, locale, dot);
    append_row(&form, &labels[1], "Color", &colors, locale);

    let creation = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let checks = [("Events", calendar.events), ("Tasks", calendar.tasks)].map(|(label, active)| {
        let check = gtk::CheckButton::with_label(locale.translate(label));
        check.set_active(active);
        creation.append(&check);
        check
    });
    append_row(&form, &labels[2], "Creation", &creation, locale);

    let (alarms, personal, alarm) = alarm_controls(calendar, locale);
    append_row(&form, &labels[3], "Alarms", &alarms, locale);
    let [events, tasks] = checks;
    CalendarForm {
        widget: form,
        folder,
        background,
        foreground,
        events,
        tasks,
        personal,
        alarm,
    }
}

fn color_controls(
    calendar: &Calendar,
    locale: &dyn Locale,
    dot: &gtk::DrawingArea,
) -> (gtk::Grid, gtk::ColorDialogButton, gtk::ColorDialogButton) {
    let colors = gtk::Grid::builder()
        .column_spacing(8)
        .row_spacing(6)
        .build();
    let background = color_button(&calendar.background, "Background", locale);
    let foreground = color_button(&calendar.foreground, "Foreground", locale);
    let settings_labels = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
    let previews = Rc::new(ColorPreviews::new(locale, dot));
    for (index, (label, button, preview_label, preview)) in [
        (
            "Background",
            &background,
            "Preview (timed)",
            &previews.timed,
        ),
        (
            "Foreground",
            &foreground,
            "Preview (all-day)",
            &previews.all_day,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        append_color_row(
            &colors,
            index as i32,
            (label, button),
            (preview_label, preview),
            &settings_labels,
            locale,
        );
    }
    previews.update(background.rgba(), foreground.rgba());
    previews.connect(&background, &foreground);
    (colors, background, foreground)
}

fn append_color_row(
    grid: &gtk::Grid,
    row: i32,
    setting: (&str, &gtk::ColorDialogButton),
    preview: (&str, &gtk::DrawingArea),
    settings_labels: &gtk::SizeGroup,
    locale: &dyn Locale,
) {
    let settings = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let setting_label = gtk::Label::builder()
        .label(locale.translate(setting.0))
        .xalign(0.0)
        .build();
    settings_labels.add_widget(&setting_label);
    settings.append(&setting_label);
    settings.append(setting.1);
    grid.attach(&settings, 0, row, 1, 1);

    let preview_label = gtk::Label::builder()
        .label(format!("{}:", locale.translate(preview.0)))
        .xalign(0.0)
        .valign(gtk::Align::Center)
        .margin_start(24)
        .build();
    preview_label.add_css_class("ev_management_preview_label");
    grid.attach(&preview_label, 1, row, 1, 1);
    preview.1.set_halign(gtk::Align::Start);
    preview.1.set_valign(gtk::Align::Center);
    grid.attach(preview.1, 2, row, 1, 1);
}

struct ColorPreviews {
    dot: gtk::DrawingArea,
    timed: gtk::DrawingArea,
    all_day: gtk::DrawingArea,
    title: String,
}

impl ColorPreviews {
    fn new(locale: &dyn Locale, dot: &gtk::DrawingArea) -> Self {
        let timed = gtk::DrawingArea::builder()
            .width_request(22)
            .height_request(22)
            .build();
        let all_day = gtk::DrawingArea::builder().height_request(20).build();
        let title = locale.translate("Title").to_owned();
        let title_width = all_day.create_pango_layout(Some(&title)).pixel_size().0;
        all_day.set_width_request(title_width + 8);
        label_control(&timed, "Preview (timed)", locale);
        label_control(&all_day, "Preview (all-day)", locale);

        Self {
            dot: dot.clone(),
            timed,
            all_day,
            title,
        }
    }

    fn update(&self, bg: gdk::RGBA, fg: gdk::RGBA) {
        set_dot_color(&self.dot, bg);
        set_dot_color(&self.timed, bg);
        let title = self.title.clone();
        self.all_day
            .set_draw_func(move |_, context, width, height| {
                context.set_source_rgba(bg.red().into(), bg.green().into(), bg.blue().into(), 1.0);
                context.rectangle(0.0, 0.0, width.into(), height.into());
                let _ = context.fill();
                context.set_source_rgba(fg.red().into(), fg.green().into(), fg.blue().into(), 1.0);
                context.set_font_size(12.0);
                context.move_to(4.0, f64::from(height) / 2.0 + 4.0);
                let _ = context.show_text(&title);
            });
        self.all_day.queue_draw();
    }

    fn connect(
        self: &Rc<Self>,
        background: &gtk::ColorDialogButton,
        foreground: &gtk::ColorDialogButton,
    ) {
        let previews = self.clone();
        let other = foreground.downgrade();
        background.connect_rgba_notify(move |button| {
            if let Some(other) = other.upgrade() {
                previews.update(button.rgba(), other.rgba());
            }
        });
        let previews = self.clone();
        let other = background.downgrade();
        foreground.connect_rgba_notify(move |button| {
            if let Some(other) = other.upgrade() {
                previews.update(other.rgba(), button.rgba());
            }
        });
    }
}

fn alarm_controls(
    calendar: &Calendar,
    locale: &dyn Locale,
) -> (gtk::Box, gtk::ToggleButton, Rc<AlarmInput>) {
    let alarms = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let modes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    modes.add_css_class("linked");
    let calendar_mode = gtk::ToggleButton::with_label(locale.translate("Calendar"));
    let personal_mode = gtk::ToggleButton::with_label(locale.translate("Personal"));
    personal_mode.set_group(Some(&calendar_mode));
    modes.append(&calendar_mode);
    modes.append(&personal_mode);
    alarms.append(&modes);
    let input = Rc::new(AlarmInput::for_calendar(
        locale,
        calendar.default_alarm.as_ref(),
    ));
    input.widget.set_visible(calendar.personal_alarms);
    alarms.append(&input.widget);
    personal_mode.set_active(calendar.personal_alarms);
    calendar_mode.set_active(!calendar.personal_alarms);
    let widget = input.widget.clone();
    personal_mode.connect_toggled(move |button| widget.set_visible(button.is_active()));
    (alarms, personal_mode, input)
}

fn append_row(
    form: &gtk::Box,
    label: &gtk::Label,
    title: &str,
    content: &impl IsA<gtk::Widget>,
    locale: &dyn Locale,
) {
    if form.first_child().is_some() {
        let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        separator.add_css_class("ev_management_form_separator");
        form.append(&separator);
    }
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if let Some(previous) = label.parent().and_downcast::<gtk::Box>() {
        previous.remove(label);
    }
    label.set_mnemonic_widget(Some(content));
    label_control(content, title, locale);
    row.append(label);
    row.append(content);
    form.append(&row);
}

fn label_control(widget: &impl IsA<gtk::Widget>, title: &str, locale: &dyn Locale) {
    widget.set_tooltip_text(Some(locale.translate(title)));
    widget
        .as_ref()
        .update_property(&[gtk::accessible::Property::Label(locale.translate(title))]);
}

fn color(value: &str) -> gdk::RGBA {
    let mut rgba = value.parse::<gdk::RGBA>().unwrap_or(gdk::RGBA::BLACK);
    rgba.set_alpha(1.0);
    rgba
}

fn color_button(value: &str, title: &str, locale: &dyn Locale) -> gtk::ColorDialogButton {
    let dialog = gtk::ColorDialog::builder()
        .title(locale.translate(title))
        .with_alpha(false)
        .build();
    let button = gtk::ColorDialogButton::new(Some(dialog));
    button.set_rgba(&color(value));
    label_control(&button, title, locale);
    button
}
