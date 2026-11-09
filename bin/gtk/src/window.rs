// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::Cell;

use chrono::NaiveDate;
use gtk::{gio, glib, prelude::*, subclass::prelude::*};

use crate::view::{card::EventCard, details::EventDetails, page::CalendarPage};

#[derive(Clone, Copy)]
struct WindowState {
    size: (i32, i32),
    maximized: bool,
}

mod imp {
    use super::*;

    resource_template! { "window.ui",
    pub struct Window {
        #[template_child]
        pub page: gtk::TemplateChild<CalendarPage>,
        pub(super) hidden_state: Cell<Option<WindowState>>,
    }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "EventixWindow";
        type Type = super::Window;
        type ParentType = gtk::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            CalendarPage::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Window {
        fn dispose(&self) {
            self.dispose_template();
        }
    }

    impl WidgetImpl for Window {
        fn move_focus(&self, direction: gtk::DirectionType) {
            // Unhandled arrows reach GTK's window bindings after the focused control.
            // Do not let that fallback move focus out of the active popup.
            if self.page.popup_open()
                && matches!(
                    direction,
                    gtk::DirectionType::Left
                        | gtk::DirectionType::Right
                        | gtk::DirectionType::Up
                        | gtk::DirectionType::Down
                )
            {
                return;
            }

            self.parent_move_focus(direction);
        }
    }

    impl WindowImpl for Window {
        fn close_request(&self) -> glib::Propagation {
            self.page.stop();
            self.parent_close_request()
        }
    }

    impl ApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
            gtk::Native, gtk::Root, gtk::ShortcutManager, gio::ActionGroup, gio::ActionMap;
}

impl Window {
    /// Hides the window while retaining its size and maximized state.
    pub fn hide_to_tray(&self) {
        if self.is_visible() {
            self.imp().hidden_state.set(Some(WindowState {
                size: self.default_size(),
                maximized: self.is_maximized(),
            }));
            self.set_visible(false);
        }
    }

    /// Presents the window, restoring its state if it was hidden to the tray.
    pub fn present_from_tray(&self) {
        if let Some(state) = self.imp().hidden_state.take() {
            self.set_default_size(state.size.0, state.size.1);
            self.set_maximized(state.maximized);
        }
        self.present();
    }

    pub fn setup_tray(&self) {
        self.imp().page.install_tray(self);
    }

    pub fn new(app: &gtk::Application, prefix: String, date: Option<NaiveDate>) -> Self {
        let obj = Self::build(app);
        obj.imp().page.start(prefix, date);

        obj
    }

    fn build(app: &gtk::Application) -> Self {
        let obj: Self = glib::Object::builder().property("application", app).build();
        obj.setup_key_controller();
        obj.connect_popover_dismissal();
        obj.setup_actions(app);

        obj
    }

    #[cfg(test)]
    pub(crate) fn test_new(app: &gtk::Application) -> (Self, CalendarPage) {
        let window = Self::build(app);
        let page = window.imp().page.get();

        (window, page)
    }

    fn setup_key_controller(&self) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);

        let weak = self.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(window) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };

            window.handle_key_pressed(key, modifiers)
        });

        self.add_controller(keys);
    }

    fn handle_key_pressed(
        &self,
        key: gtk::gdk::Key,
        modifiers: gtk::gdk::ModifierType,
    ) -> glib::Propagation {
        // Button-only date inputs have an entry beside the calendar inside their popup.
        if let Some(focus) = GtkWindowExt::focus(self)
            && let Some(entry) = focus
                .ancestor(gtk::Entry::static_type())
                .and_downcast::<gtk::Entry>()
            && entry.ancestor(gtk::Popover::static_type()).is_some()
            && let Some(picker) = entry
                .parent()
                .and_then(|parent| parent.last_child())
                .and_downcast::<crate::widgets::DatePicker>()
        {
            let propagation = picker.entry_key_pressed(&entry, key, modifiers);
            if propagation == glib::Propagation::Stop {
                return propagation;
            }
        }

        if let Some(focus) = GtkWindowExt::focus(self)
            && let Some(input) = focus
                .ancestor(crate::widgets::DateInput::static_type())
                .and_downcast::<crate::widgets::DateInput>()
        {
            let propagation = input.key_pressed(key, modifiers);
            if propagation == glib::Propagation::Stop {
                return propagation;
            }
        }

        if let Some(focus) = GtkWindowExt::focus(self)
            && let Some(input) = focus
                .ancestor(crate::widgets::TimeInput::static_type())
                .and_downcast::<crate::widgets::TimeInput>()
        {
            let propagation = input.key_pressed(key, modifiers);
            if propagation == glib::Propagation::Stop {
                return propagation;
            }
        }

        if let Some(focus) = GtkWindowExt::focus(self)
            && let Some(picker) = focus
                .clone()
                .downcast::<crate::widgets::MonthPicker>()
                .ok()
                .or_else(|| {
                    focus
                        .ancestor(crate::widgets::MonthPicker::static_type())
                        .and_downcast::<crate::widgets::MonthPicker>()
                })
        {
            let propagation = picker.key_pressed(key, modifiers);
            if propagation == glib::Propagation::Stop {
                return propagation;
            }
        }

        if let Some(focus) = GtkWindowExt::focus(self)
            && let Some(picker) = focus
                .clone()
                .downcast::<crate::widgets::DatePicker>()
                .ok()
                .or_else(|| {
                    focus
                        .ancestor(crate::widgets::DatePicker::static_type())
                        .and_downcast::<crate::widgets::DatePicker>()
                })
        {
            let propagation = picker.key_pressed(key, modifiers);
            if propagation == glib::Propagation::Stop {
                return propagation;
            }
        }

        if let Some(propagation) = self.imp().page.handle_editor_key(key, modifiers) {
            return propagation;
        }

        if let Some(propagation) = self.handle_details_key(key) {
            return propagation;
        }

        if key == gtk::gdk::Key::Escape {
            self.imp().page.dismiss_popovers();
            return glib::Propagation::Proceed;
        }

        if modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK)
            && !modifiers
                .intersects(gtk::gdk::ModifierType::ALT_MASK | gtk::gdk::ModifierType::SUPER_MASK)
        {
            match key {
                gtk::gdk::Key::Left => {
                    self.imp().page.navigate(-1);
                    return glib::Propagation::Stop;
                }
                gtk::gdk::Key::Right => {
                    self.imp().page.navigate(1);
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
        }

        if modifiers.intersects(
            gtk::gdk::ModifierType::ALT_MASK
                | gtk::gdk::ModifierType::CONTROL_MASK
                | gtk::gdk::ModifierType::SUPER_MASK,
        ) {
            return glib::Propagation::Proceed;
        }

        let forward = match key {
            gtk::gdk::Key::Right | gtk::gdk::Key::Down => true,
            gtk::gdk::Key::Left | gtk::gdk::Key::Up => false,
            _ => return glib::Propagation::Proceed,
        };

        if self.imp().page.popup_open() {
            return glib::Propagation::Proceed;
        }

        let Some(card) = self.focused_event_card() else {
            return glib::Propagation::Proceed;
        };

        self.imp().page.focus_event(Some(&card), forward);

        glib::Propagation::Stop
    }

    /// Returns key propagation for focused event details, or `None` when details are not focused.
    fn handle_details_key(&self, key: gtk::gdk::Key) -> Option<glib::Propagation> {
        let mut focused = gtk::prelude::GtkWindowExt::focus(self);
        while let Some(widget) = focused {
            if widget.is::<EventDetails>() {
                // Let popup text and scrolling handle their own navigation keys.
                if key == gtk::gdk::Key::Escape {
                    // A nested menu gets first refusal; Escape otherwise closes details.
                    let mut child = gtk::prelude::GtkWindowExt::focus(self);
                    while let Some(widget) = child {
                        if widget.is::<gtk::Popover>() && !widget.is::<EventDetails>() {
                            return Some(glib::Propagation::Proceed);
                        }

                        child = widget.parent();
                    }

                    self.imp().page.dismiss_popovers();
                    return Some(glib::Propagation::Stop);
                }

                return Some(glib::Propagation::Proceed);
            }

            focused = widget.parent();
        }

        None
    }

    fn focused_event_card(&self) -> Option<EventCard> {
        let mut focused = gtk::prelude::GtkWindowExt::focus(self);
        while let Some(widget) = focused {
            if let Some(card) = widget.downcast_ref::<EventCard>() {
                return Some(card.clone());
            }

            focused = widget.parent();
        }

        None
    }

    fn connect_popover_dismissal(&self) {
        track_popup_dismissal(self.upcast_ref(), &self.imp().page);
    }

    fn setup_actions(&self, app: &gtk::Application) {
        let action = gio::SimpleAction::new("search", None);
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(page) = page.upgrade() {
                page.focus_search();
            }
        });
        self.add_action(&action);
        app.set_accels_for_action("win.search", &["<Primary>f"]);

        let action = gio::SimpleAction::new("new-event", None);
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(page) = page.upgrade() {
                page.new_event();
            }
        });
        self.add_action(&action);
        app.set_accels_for_action("win.new-event", &["<Control>e"]);

        let action = gio::SimpleAction::new("new-task", None);
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(page) = page.upgrade() {
                page.new_task();
            }
        });
        self.add_action(&action);
        app.set_accels_for_action("win.new-task", &["<Control>t"]);

        let action = gio::SimpleAction::new("quick-add-task", None);
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(page) = page.upgrade() {
                page.focus_quick_task();
            }
        });
        self.add_action(&action);
        app.set_accels_for_action("win.quick-add-task", &["<Control>a"]);

        for (name, keys, days) in [
            ("previous", vec!["<Control>Left"], Some(-1)),
            ("next", vec!["<Control>Right"], Some(1)),
            ("today", vec!["<Control>Home", "<Control>period"], None),
            ("reload", vec!["<Primary>r", "F5"], None),
            ("week-view", vec!["<Control>w"], None),
            ("month-view", vec!["<Control>m"], None),
            ("list-view", vec!["<Control>l"], None),
        ] {
            let action = gio::SimpleAction::new(name, None);
            let page = self.imp().page.downgrade();
            action.connect_activate(move |_, _| {
                if let Some(page) = page.upgrade() {
                    if page.editor_open() {
                        return;
                    }

                    if let Some(days) = days {
                        page.navigate(days);
                    } else if name == "reload" {
                        page.reload();
                    } else if name == "week-view" {
                        page.switch_to_week();
                    } else if name == "month-view" {
                        page.switch_to_month();
                    } else if name == "list-view" {
                        page.switch_to_list();
                    } else {
                        page.show_today();
                    }
                }
            });
            self.add_action(&action);
            app.set_accels_for_action(&format!("win.{name}"), &keys);
        }

        let action = gio::SimpleAction::new("show-week", Some(glib::VariantTy::STRING));
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, parameter| {
            if let Some(page) = page.upgrade()
                && let Some(date) = parameter.and_then(|p| p.str())
                && let Ok(date) = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            {
                page.show_week_at(date);
            }
        });
        self.add_action(&action);
    }
}

/// Dismisses popovers inside Eventix and keeps their visibility with the active owner.
pub(crate) fn track_popup_dismissal(window: &gtk::Window, page: &CalendarPage) {
    let click = gtk::GestureClick::new();
    click.set_button(0);
    click.set_propagation_phase(gtk::PropagationPhase::Capture);

    let weak_window = window.downgrade();
    let weak_page = page.downgrade();
    click.connect_pressed(move |click, _, _, _| {
        if let (Some(window), Some(page), Some(surface)) = (
            weak_window.upgrade(),
            weak_page.upgrade(),
            click.current_event().and_then(|event| event.surface()),
        ) {
            dismiss_popups_on_surface(&window, &page, &surface);
        }
    });
    window.add_controller(click);

    let weak_page = page.downgrade();
    window.connect_hide(move |_| {
        if let Some(page) = weak_page.upgrade() {
            page.set_popup_owner_visible(false);
        }
    });

    let weak_page = page.downgrade();
    window.connect_map(move |window| {
        if let Some(page) = weak_page.upgrade() {
            sync_popup_visibility(window, &page);
        }
    });

    let weak_page = page.downgrade();
    window.connect_is_active_notify(move |window| {
        if let Some(page) = weak_page.upgrade() {
            sync_popup_visibility(window, &page);
        }
    });

    let weak_page = page.downgrade();
    window.connect_realize(move |window| {
        let Some(surface) = window.surface().and_downcast::<gtk::gdk::Toplevel>() else {
            return;
        };

        let weak_page = weak_page.clone();
        let weak_window = window.downgrade();
        surface.connect_state_notify(move |_| {
            if let (Some(window), Some(page)) = (weak_window.upgrade(), weak_page.upgrade()) {
                sync_popup_visibility(&window, &page);
            }
        });
    });
}

fn sync_popup_visibility(window: &gtk::Window, page: &CalendarPage) {
    // Only nested popup grabs may override GTK's inactive state. On X11 the native
    // FOCUSED flag describes focused decorations, which can outlive keyboard focus.
    let focused = window.is_active()
        || (crate::view::editor::has_nested_popup(window)
            && window
                .surface()
                .and_downcast::<gtk::gdk::Toplevel>()
                .is_some_and(|surface| surface.state().contains(gtk::gdk::ToplevelState::FOCUSED)));

    page.set_popup_owner_visible(window.is_visible() && focused);
}

pub(crate) fn dismiss_popups_on_surface(
    window: &gtk::Window,
    page: &CalendarPage,
    surface: &gtk::gdk::Surface,
) {
    // Popovers and nested menus have native surfaces of their own. Their controls must
    // remain usable, while every click on the main surface dismisses the current popup.
    if window.surface().as_ref() == Some(surface) {
        page.dismiss_popovers();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn settle() {
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(200)));
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn tray_restores_window_size_and_maximized_state() {
        gtk::init().unwrap();
        gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.hrniels.Eventix.TrayTest")
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let (window, _) = Window::test_new(&app);
        window.set_default_size(1000, 700);
        window.present();
        settle();

        let size = window.default_size();
        window.hide_to_tray();
        assert!(!window.is_visible());
        window.set_default_size(800, 600);
        window.hide_to_tray();
        window.present_from_tray();
        settle();
        assert_eq!(window.default_size(), size);
        assert!(!window.is_maximized());

        window.maximize();
        settle();
        assert!(window.is_maximized());
        window.hide_to_tray();
        settle();
        window.unmaximize();
        window.present_from_tray();
        settle();
        assert!(window.is_maximized());

        window.unmaximize();
        settle();
        assert_eq!(window.default_size(), size);
        window.hide_to_tray();
        window.present_from_tray();
        settle();
        assert!(!window.is_maximized());
        assert_eq!(window.default_size(), size);
        window.destroy();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn date_picker_shortcuts_take_precedence_over_window_navigation() {
        gtk::init().unwrap();
        gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.hrniels.Eventix.DatePickerTest")
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let (window, page) = Window::test_new(&app);
        window.present();
        settle();

        let input = crate::widgets::DateInput::new(
            eventix_locale::default().as_ref(),
            "2024-03-31".parse().unwrap(),
        );
        page.append(&input);
        input.entry().grab_focus();
        settle();
        assert_eq!(
            window.handle_key_pressed(gtk::gdk::Key::Left, gtk::gdk::ModifierType::CONTROL_MASK),
            glib::Propagation::Stop
        );
        assert_eq!(input.entry().text(), "2024-03-30");
        assert_eq!(
            window.handle_key_pressed(gtk::gdk::Key::Return, gtk::gdk::ModifierType::empty()),
            glib::Propagation::Stop
        );
        assert_eq!(input.entry().text(), "2024-03-30");
        page.remove(&input);

        let quick = page.imp().sidebar.quick_task();
        quick.widget.set_visible(true);
        let entry = quick
            .due
            .picker()
            .popover()
            .unwrap()
            .child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        entry.set_text("2024-03-31");
        quick.due.picker().popup();
        settle();
        entry.grab_focus();
        settle();
        assert_eq!(
            window.handle_key_pressed(gtk::gdk::Key::Left, gtk::gdk::ModifierType::CONTROL_MASK),
            glib::Propagation::Stop
        );
        assert_eq!(entry.text(), "2024-03-30");
        assert_eq!(
            window.handle_key_pressed(gtk::gdk::Key::Return, gtk::gdk::ModifierType::empty()),
            glib::Propagation::Stop
        );
        assert_eq!(entry.text(), "2024-03-30");
        assert!(!quick.due.picker().is_active());
        quick.due.picker().popup();
        settle();
        entry.grab_focus();
        entry.set_text("");
        assert_eq!(
            window.handle_key_pressed(gtk::gdk::Key::Return, gtk::gdk::ModifierType::empty()),
            glib::Propagation::Stop
        );
        assert!(entry.text().is_empty());
        assert!(!quick.due.picker().is_active());

        page.switch_to_week();
        for (key, expected) in [
            (gtk::gdk::Key::Left, "2024-02-29"),
            (gtk::gdk::Key::Right, "2024-04-30"),
            (gtk::gdk::Key::Down, "2023-03-31"),
            (gtk::gdk::Key::Up, "2025-03-31"),
        ] {
            let date = "2024-03-31".parse().unwrap();
            page.imp().calendar.select_day(date);
            page.imp().picker.popup();
            settle();
            assert!(page.imp().calendar.has_focus());
            assert_eq!(
                window.handle_key_pressed(key, gtk::gdk::ModifierType::ALT_MASK),
                glib::Propagation::Stop
            );
            assert!(page.imp().picker.is_active());
            assert_eq!(page.imp().calendar.date(), date);
            assert_eq!(
                window.handle_key_pressed(gtk::gdk::Key::Return, gtk::gdk::ModifierType::empty()),
                glib::Propagation::Stop
            );
            assert_eq!(
                page.imp().calendar.date(),
                expected.parse::<NaiveDate>().unwrap()
            );
            assert!(!page.imp().picker.is_active());
        }

        let week_size = (page.imp().calendar.width(), page.imp().calendar.height());
        page.switch_to_month();
        for (key, expected) in [
            (gtk::gdk::Key::Left, "2024-02-01"),
            (gtk::gdk::Key::Right, "2024-04-01"),
            (gtk::gdk::Key::Up, "2023-03-01"),
            (gtk::gdk::Key::Down, "2025-03-01"),
        ] {
            let date = "2024-03-31".parse().unwrap();
            page.imp().month_picker.select_month(date);
            page.imp().picker.popup();
            settle();
            assert!(page.imp().month_picker.has_focus());
            assert!(!page.imp().calendar.is_mapped());
            assert_eq!(
                (
                    page.imp().month_picker.width(),
                    page.imp().month_picker.height()
                ),
                week_size,
            );
            assert_eq!(
                window.handle_key_pressed(key, gtk::gdk::ModifierType::CONTROL_MASK),
                glib::Propagation::Stop
            );
            assert_eq!(
                page.imp().month_picker.date(),
                "2024-03-01".parse::<NaiveDate>().unwrap()
            );
            window.handle_key_pressed(gtk::gdk::Key::Return, gtk::gdk::ModifierType::empty());
            assert_eq!(
                page.imp().month_picker.date(),
                expected.parse::<NaiveDate>().unwrap()
            );
            assert!(!page.imp().picker.is_active());
        }
        window.destroy();
    }
}
