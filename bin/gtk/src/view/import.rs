// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::Cell, rc::Rc};

use eventix_ical::objects::{CalCompType, CalComponent, EventLike};
use eventix_locale::Locale;
use gtk::{glib, prelude::*};

use super::error::{ErrorMessage, show_error};
use crate::model::editor::EditorCalendar;
use crate::{model::import::PreparedImport, widgets::CalendarSelector};

/// The standalone importer's preview, hosted as a child of the main application window.
pub(super) struct ImportView {
    pub window: gtk::Window,
    import: gtk::Button,
    cancel: gtk::Button,
    selector: CalendarSelector,
    error: ErrorMessage,
    busy: Cell<bool>,
}

impl ImportView {
    pub fn new(
        parent: &gtk::Window,
        locale: &dyn Locale,
        preview: &PreparedImport,
        submit: impl Fn(String) + 'static,
        closed: impl Fn() + 'static,
    ) -> Rc<Self> {
        let window = Self::create_window(parent, locale);
        let content = Self::create_content(locale, preview);
        window.set_child(Some(&content));

        let (calendar_row, selector) = Self::create_calendar_row(locale, preview);
        content.append(&calendar_row);
        let error = ErrorMessage::new();
        content.append(&error.widget);
        let (buttons, import, cancel) = Self::create_buttons(locale);
        import.set_sensitive(!preview.calendars.is_empty());
        content.append(&buttons);

        let view = Rc::new(Self {
            window,
            import,
            cancel,
            selector,
            error,
            busy: Cell::new(false),
        });
        if preview.calendars.is_empty() {
            view.show_error(locale.translate("error.import_no_calendar"), locale);
        }
        view.connect_submit(submit);
        view.connect_close(closed);
        view
    }

    fn create_window(parent: &gtk::Window, locale: &dyn Locale) -> gtk::Window {
        let window = gtk::Window::builder()
            .title(locale.translate("Importer"))
            .transient_for(parent)
            .modal(true)
            .destroy_with_parent(true)
            .build();
        window.set_application(parent.application().as_ref());
        window
    }

    fn create_content(locale: &dyn Locale, preview: &PreparedImport) -> gtk::Box {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        let heading = gtk::Label::new(Some(&format!(
            "{}:",
            locale.translate("Events/tasks to import")
        )));
        heading.set_xalign(0.0);
        content.append(&heading);
        content.append(&Self::create_preview(locale, preview));
        content
    }

    fn create_preview(locale: &dyn Locale, preview: &PreparedImport) -> gtk::ScrolledWindow {
        let list = gtk::Box::new(gtk::Orientation::Vertical, 12);
        for component in preview
            .calendar
            .components()
            .iter()
            .filter(|c| c.rid().is_none())
        {
            list.append(&Self::create_component_row(locale, preview, component));
        }
        gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(true)
            .propagate_natural_height(true)
            .max_content_width(600)
            .max_content_height(400)
            .child(&list)
            .build()
    }

    fn create_component_row(
        locale: &dyn Locale,
        preview: &PreparedImport,
        component: &CalComponent,
    ) -> gtk::Box {
        let icon = match component.ctype() {
            CalCompType::Event => resource_path!("icons/calendar-event-symbolic.svg"),
            CalCompType::Todo => resource_path!("icons/card-heading-symbolic.svg"),
        };
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let image = gtk::Image::from_resource(icon);
        image.set_pixel_size(25);
        image.set_valign(gtk::Align::Start);
        row.append(&image);
        let label = gtk::Label::builder()
            .xalign(0.0)
            .use_markup(true)
            .label(Self::component_text(locale, preview, component))
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .max_width_chars(60)
            .hexpand(true)
            .valign(gtk::Align::Start)
            .build();
        row.append(&label);
        row
    }

    fn component_text(
        locale: &dyn Locale,
        preview: &PreparedImport,
        component: &CalComponent,
    ) -> String {
        let mut text = String::new();
        if let Some(summary) = component.summary() {
            text.push_str(&format!("{}\n", glib::markup_escape_text(summary)));
        }
        let dates = locale.date_range(
            component.start().cloned(),
            component.end_or_due().cloned(),
            &preview.calendar.date_context(),
            locale.timezone(),
        );
        text.push_str(&glib::markup_escape_text(&dates));
        if let Some(rule) = component.rrule() {
            text.push_str(&format!(
                "\n{}",
                glib::markup_escape_text(&rule.human(locale).to_string())
            ));
        }
        if let Some((_, _, name)) = preview
            .duplicates
            .iter()
            .find(|(uid, _, _)| uid == component.uid())
        {
            let warning =
                formatx::formatx!(locale.translate("warning.import_exists_overwrite"), name)
                    .unwrap();
            text.push_str(&format!("\n<b>{}</b>", glib::markup_escape_text(&warning)));
        }
        text
    }

    fn create_calendar_row(
        locale: &dyn Locale,
        preview: &PreparedImport,
    ) -> (gtk::Box, CalendarSelector) {
        let calendars: Vec<_> = preview
            .calendars
            .iter()
            .map(|calendar| EditorCalendar {
                id: calendar.id.clone(),
                name: calendar.name.clone(),
                background: calendar.color.clone(),
                personal: false,
            })
            .collect();
        let selector = CalendarSelector::new(&calendars, None, false);
        selector.widget.set_hexpand(true);
        let calendar_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        calendar_row.append(&gtk::Label::new(Some(&format!(
            "{}:",
            locale.translate("Calendar")
        ))));
        calendar_row.append(&selector.widget);
        (calendar_row, selector)
    }

    fn create_buttons(locale: &dyn Locale) -> (gtk::Box, gtk::Button, gtk::Button) {
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        let import = action_button(
            locale.translate("Import"),
            resource_path!("icons/database-add-symbolic.svg"),
        );
        let cancel = action_button(
            locale.translate("Cancel"),
            resource_path!("icons/x-circle-symbolic.svg"),
        );
        buttons.append(&import);
        buttons.append(&cancel);
        (buttons, import, cancel)
    }

    fn connect_submit(self: &Rc<Self>, submit: impl Fn(String) + 'static) {
        let weak = Rc::downgrade(self);
        self.import.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade()
                && !view.busy.get()
                && let Some(calendar) = view.selector.selected_id()
            {
                view.set_busy(true);
                submit(calendar);
            }
        });
    }

    fn connect_close(self: &Rc<Self>, closed: impl Fn() + 'static) {
        let weak = self.window.downgrade();
        self.cancel.connect_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                window.close();
            }
        });
        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if weak.upgrade().is_some_and(|view| view.busy.get()) {
                return glib::Propagation::Stop;
            }
            closed();
            glib::Propagation::Proceed
        });
        let keys = gtk::EventControllerKey::new();
        let weak = self.window.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(window) = weak.upgrade() {
                    window.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.window.add_controller(keys);
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.import
            .set_sensitive(!busy && self.selector.selected_id().is_some());
        self.cancel.set_sensitive(!busy);
        self.selector.widget.set_sensitive(!busy);
    }

    pub fn finished(&self, result: Result<(), String>, locale: &dyn Locale) {
        self.set_busy(false);
        match result {
            Ok(()) => self.window.close(),
            Err(error) => self.show_error(&error, locale),
        }
    }

    fn show_error(&self, error: &str, locale: &dyn Locale) {
        show_error(&self.error, error, locale.translate("Error"));
    }
}

fn action_button(label: &str, resource: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let image = gtk::Image::from_resource(resource);
    image.set_pixel_size(16);
    content.append(&image);
    content.append(&gtk::Label::new(Some(label)));
    gtk::Button::builder().child(&content).build()
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc, sync::Arc};

    use eventix_ical::{col::CalDir, objects::CalCompType};
    use eventix_state::{CalendarSettings, CollectionSettings, State, SyncerType};
    use gtk::{gio, prelude::*};

    use super::ImportView;
    use crate::worker::import::{apply, prepare};

    #[test]
    #[ignore = "requires a GTK display"]
    fn import_dialog_is_transient_modal_retryable_and_does_not_close_parent() {
        gtk::init().unwrap();
        gio::resources_register_include!("eventix.gresource").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let mut state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        state.store_mut().add(CalDir::new_empty(
            Arc::new("test".into()),
            tmp.path().into(),
            "<Calendar> & name".into(),
            false,
        ));
        let mut collection = CollectionSettings::new(SyncerType::FileSystem {
            path: tmp.path().display().to_string(),
        });
        let mut settings = CalendarSettings::default();
        settings.set_enabled(true);
        settings.set_name("<Calendar> & name".into());
        settings.set_types(vec![CalCompType::Event]);
        collection
            .all_calendars_mut()
            .insert("test".into(), settings);
        state
            .settings_mut()
            .collections_mut()
            .insert("local".into(), collection);
        let input = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:test\r\n\
            DTSTART:20261109T090000Z\r\nDTEND:20261109T100000Z\r\n\
            SUMMARY:<Meeting> & friends\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let initial = prepare(&state, input).unwrap();
        apply(&initial, &mut state, "test").unwrap();
        let preview = prepare(&state, input).unwrap();
        let app = gtk::Application::builder()
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let parent = gtk::ApplicationWindow::builder().application(&app).build();
        parent.present();
        let submissions = Rc::new(Cell::new(0));
        let observed = submissions.clone();
        let closed = Rc::new(Cell::new(false));
        let observed_close = closed.clone();
        let view = ImportView::new(
            parent.upcast_ref(),
            state.locale().as_ref(),
            &preview,
            move |calendar| {
                assert_eq!(calendar, "test");
                observed.set(observed.get() + 1);
            },
            move || observed_close.set(true),
        );
        view.window.present();
        assert_eq!(view.window.title().as_deref(), Some("Importer"));
        assert!(view.window.is_modal());
        assert_eq!(
            view.window.transient_for().as_ref(),
            Some(parent.upcast_ref())
        );
        assert_eq!(view.window.application(), Some(app));
        let content = view.window.child().unwrap();
        let scroll = content
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap();
        let viewport = scroll.child().unwrap();
        let list = viewport.first_child().unwrap();
        let row = list.first_child().unwrap();
        let image = row.first_child().unwrap().downcast::<gtk::Image>().unwrap();
        assert!(
            image.paintable().is_some(),
            "event icon must load from resources"
        );
        for button in [&view.import, &view.cancel] {
            let image = button
                .child()
                .unwrap()
                .first_child()
                .unwrap()
                .downcast::<gtk::Image>()
                .unwrap();
            assert!(
                image.paintable().is_some(),
                "action icon must load from resources"
            );
        }
        let label = row.last_child().unwrap().downcast::<gtk::Label>().unwrap();
        assert!(label.text().contains("<Meeting> & friends"));
        assert!(label.text().contains("09:00 ‒ 10:00"));
        assert!(!label.text().contains("&#"));
        assert!(label.text().contains("<Calendar> & name"));
        view.import.emit_clicked();
        view.import.emit_clicked();
        assert_eq!(submissions.get(), 1);
        view.window.close();
        assert!(
            !closed.get(),
            "closing while saving must not lose the result"
        );
        view.finished(Err("Retryable failure".into()), state.locale().as_ref());
        assert!(view.error.widget.is_visible());
        assert!(view.import.is_sensitive());
        view.import.emit_clicked();
        assert_eq!(submissions.get(), 2);
        view.finished(Ok(()), state.locale().as_ref());
        assert!(closed.get());
        assert!(parent.is_visible());
        parent.destroy();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn import_dialog_without_destination_disables_import_but_allows_cancel() {
        gtk::init().unwrap();
        gio::resources_register_include!("eventix.gresource").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let xdg = Arc::new(eventix_state::with_test_xdg(tmp.path(), tmp.path()));
        let state = State::new_with_timezone(xdg, Some(chrono_tz::UTC)).unwrap();
        let preview = prepare(&state,
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:test\r\nSUMMARY:Task\r\nEND:VTODO\r\nEND:VCALENDAR\r\n").unwrap();
        let parent = gtk::Window::new();
        parent.present();
        let closed = Rc::new(Cell::new(false));
        let observed = closed.clone();
        let view = ImportView::new(
            &parent,
            state.locale().as_ref(),
            &preview,
            |_| panic!("no destination"),
            move || observed.set(true),
        );
        view.window.present();
        assert!(!view.import.is_sensitive());
        assert!(view.error.widget.is_visible());
        let scroll = import_scroll(&view);
        assert_eq!(scroll.min_content_width(), -1);
        assert_eq!(scroll.min_content_height(), -1);
        assert_eq!(scroll.max_content_width(), 600);
        assert_eq!(scroll.max_content_height(), 400);
        assert!(scroll.measure(gtk::Orientation::Vertical, 400).1 < 100);
        let list = scroll.child().unwrap().first_child().unwrap();
        let image = list
            .first_child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::Image>()
            .unwrap();
        assert!(
            image.paintable().is_some(),
            "task icon must load from resources"
        );
        view.cancel.emit_clicked();
        assert!(closed.get());
        assert!(parent.is_visible());

        let components = (0..100)
            .map(|index| {
                format!("BEGIN:VTODO\r\nUID:task-{index}\r\nSUMMARY:Task {index}\r\nEND:VTODO\r\n")
            })
            .collect::<String>();
        let input = format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{components}END:VCALENDAR\r\n");
        let preview = prepare(&state, &input).unwrap();
        let long = ImportView::new(
            &parent,
            state.locale().as_ref(),
            &preview,
            |_| panic!("no destination"),
            || {},
        );
        let scroll = import_scroll(&long);
        assert_eq!(scroll.measure(gtk::Orientation::Vertical, 400).1, 400);
        long.window.destroy();
        parent.destroy();
    }

    fn import_scroll(view: &ImportView) -> gtk::ScrolledWindow {
        view.window
            .child()
            .unwrap()
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap()
    }
}
