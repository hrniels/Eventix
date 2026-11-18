// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};

use chrono::NaiveDate;
use gtk::{gio, glib, prelude::*, subclass::prelude::*};

use crate::view::{details::EventDetails, page::CalendarPage};

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
        pub(super) shortcuts: RefCell<Option<gtk::Window>>,
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
            if let Some(shortcuts) = self.shortcuts.take() {
                shortcuts.destroy();
            }
            self.dispose_template();
        }
    }

    impl WidgetImpl for Window {
        fn unmap(&self) {
            if let Some(shortcuts) = self.shortcuts.borrow().as_ref() {
                shortcuts.set_visible(false);
            }
            self.parent_unmap();
        }

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
            if self.page.editor_saving() {
                return glib::Propagation::Stop;
            }
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
    pub fn open_imports(&self, files: &[gio::File]) {
        self.present_from_tray();
        self.imp().page.open_imports(files);
    }

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

    pub fn setup_tray(&self) -> bool {
        self.imp().page.install_tray(self)
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
        if handle_input_key(self.upcast_ref(), key, modifiers) == glib::Propagation::Stop {
            return glib::Propagation::Stop;
        }

        if self.handle_app_shortcut(key, modifiers) {
            return glib::Propagation::Stop;
        }

        if let Some(propagation) = self.handle_details_key(key) {
            return propagation;
        }

        if key == gtk::gdk::Key::Escape {
            if self.imp().page.defocus_navigation_input() {
                return glib::Propagation::Stop;
            }
            self.imp().page.dismiss_popovers();
            return glib::Propagation::Proceed;
        }

        if modifiers.intersects(
            gtk::gdk::ModifierType::ALT_MASK
                | gtk::gdk::ModifierType::CONTROL_MASK
                | gtk::gdk::ModifierType::SUPER_MASK
                | gtk::gdk::ModifierType::SHIFT_MASK,
        ) {
            return glib::Propagation::Proceed;
        }

        if !self.imp().page.calendar_navigation_available() || self.focus_owns_navigation() {
            return glib::Propagation::Proceed;
        }

        match key {
            gtk::gdk::Key::Left => self.imp().page.navigate(-1),
            gtk::gdk::Key::Right => self.imp().page.navigate(1),
            gtk::gdk::Key::Home | gtk::gdk::Key::period => self.imp().page.show_today(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// Activates navigation shortcuts while preserving unrelated text input bindings.
    fn handle_app_shortcut(&self, key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> bool {
        let modifiers = modifiers & gtk::accelerator_get_default_mod_mask();
        let control = gtk::gdk::ModifierType::CONTROL_MASK;
        let previous = control | gtk::gdk::ModifierType::SHIFT_MASK;
        if matches!(key, gtk::gdk::Key::g | gtk::gdk::Key::G)
            && (modifiers == control || modifiers == previous)
        {
            let page = &self.imp().page;
            let search_focused = GtkWindowExt::focus(self)
                .is_some_and(|focus| focus.is_ancestor(&*page.imp().search));
            if !page.calendar_navigation_available()
                || (self.text_input_has_focus() && !search_focused)
            {
                return false;
            }
            let action = if modifiers == previous {
                "find-previous"
            } else {
                "find-next"
            };
            gio::prelude::ActionGroupExt::activate_action(self, action, None);
            return true;
        }

        if modifiers != control || self.text_input_has_focus() {
            return false;
        }

        let action = match key {
            gtk::gdk::Key::a => "quick-add-task",
            gtk::gdk::Key::e => "new-event",
            gtk::gdk::Key::f => "search",
            gtk::gdk::Key::t => "new-task",
            gtk::gdk::Key::w => "week-view",
            gtk::gdk::Key::m => "month-view",
            gtk::gdk::Key::l => "list-view",
            _ => return false,
        };
        gio::prelude::ActionGroupExt::activate_action(self, action, None);
        true
    }

    fn text_input_has_focus(&self) -> bool {
        let mut focused = GtkWindowExt::focus(self);
        while let Some(widget) = focused {
            if widget.is::<gtk::Editable>() || widget.is::<gtk::TextView>() {
                return true;
            }
            focused = widget.parent();
        }
        false
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

    /// Leaves editing, selection, and popup navigation to the focused control.
    fn focus_owns_navigation(&self) -> bool {
        let mut focused = gtk::prelude::GtkWindowExt::focus(self);
        while let Some(widget) = focused {
            if widget.is::<gtk::Editable>()
                || widget.is::<gtk::TextView>()
                || widget.is::<gtk::DropDown>()
                || widget.is::<gtk::Popover>()
                || widget.is::<gtk::Range>()
                || widget.is::<gtk::Calendar>()
            {
                return true;
            }

            focused = widget.parent();
        }

        false
    }

    fn connect_popover_dismissal(&self) {
        track_popup_dismissal(self.upcast_ref(), &self.imp().page);
    }

    fn setup_actions(&self, app: &gtk::Application) {
        let action = gio::SimpleAction::new("show-help-overlay", None);
        let weak = self.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(window) = weak.upgrade() {
                let locale = window.imp().page.locale();
                let title = locale.translate("Shortcuts");
                if window
                    .imp()
                    .shortcuts
                    .borrow()
                    .as_ref()
                    .is_none_or(|overlay| overlay.title().as_deref() != Some(title))
                {
                    if let Some(overlay) = window.imp().shortcuts.take() {
                        overlay.destroy();
                    }
                    let overlay = crate::view::shortcuts::build(locale.as_ref());
                    overlay.set_transient_for(Some(&window));
                    window.imp().shortcuts.replace(Some(overlay));
                }
                if let Some(overlay) = window.imp().shortcuts.borrow().as_ref() {
                    overlay.present();
                }
            }
        });
        self.add_action(&action);
        app.set_accels_for_action("win.show-help-overlay", &["<Primary>question"]);

        let action = gio::SimpleAction::new("search", None);
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(page) = page.upgrade() {
                page.focus_search();
            }
        });
        self.add_action(&action);

        for (name, forward) in [("find-next", true), ("find-previous", false)] {
            let action = gio::SimpleAction::new(name, None);
            let page = self.imp().page.downgrade();
            action.connect_activate(move |_, _| {
                if let Some(page) = page.upgrade() {
                    page.search_occurrence(forward);
                }
            });
            self.add_action(&action);
        }

        let action = gio::SimpleAction::new("new-event", None);
        self.imp().page.bind_editor_availability(&action, "enabled");
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(page) = page.upgrade() {
                page.new_event();
            }
        });
        self.add_action(&action);

        let action = gio::SimpleAction::new("new-task", None);
        self.imp().page.bind_editor_availability(&action, "enabled");
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(page) = page.upgrade() {
                page.new_task();
            }
        });
        self.add_action(&action);

        let action = gio::SimpleAction::new("quick-add-task", None);
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(page) = page.upgrade() {
                page.focus_quick_task();
            }
        });
        self.add_action(&action);

        for (name, keys, days) in [
            ("previous", vec![], Some(-1)),
            ("next", vec![], Some(1)),
            ("today", vec![], None),
            ("reload", vec!["<Primary>r", "F5"], None),
            ("week-view", vec![], None),
            ("month-view", vec![], None),
            ("list-view", vec![], None),
        ] {
            let action = gio::SimpleAction::new(name, None);
            let page = self.imp().page.downgrade();
            action.connect_activate(move |_, _| {
                if let Some(page) = page.upgrade() {
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

/// Routes date and time navigation to the focused input, including native picker popups.
pub(crate) fn handle_input_key(
    window: &gtk::Window,
    key: gtk::gdk::Key,
    modifiers: gtk::gdk::ModifierType,
) -> glib::Propagation {
    // Button-only date inputs have an entry beside the calendar inside their popup.
    if let Some(focus) = GtkWindowExt::focus(window)
        && let Some(entry) = focus
            .ancestor(gtk::Entry::static_type())
            .and_downcast::<gtk::Entry>()
        && entry.ancestor(gtk::Popover::static_type()).is_some()
        && let Some(picker) = entry
            .parent()
            .and_then(|parent| parent.last_child())
            .and_downcast::<crate::widgets::DatePicker>()
        && picker.entry_key_pressed(&entry, key, modifiers) == glib::Propagation::Stop
    {
        return glib::Propagation::Stop;
    }

    if let Some(focus) = GtkWindowExt::focus(window)
        && let Some(input) = focus
            .ancestor(crate::widgets::DateInput::static_type())
            .and_downcast::<crate::widgets::DateInput>()
        && input.key_pressed(key, modifiers) == glib::Propagation::Stop
    {
        return glib::Propagation::Stop;
    }

    if let Some(focus) = GtkWindowExt::focus(window)
        && let Some(input) = focus
            .ancestor(crate::widgets::TimeInput::static_type())
            .and_downcast::<crate::widgets::TimeInput>()
        && input.key_pressed(key, modifiers) == glib::Propagation::Stop
    {
        return glib::Propagation::Stop;
    }

    if let Some(focus) = GtkWindowExt::focus(window)
        && let Some(picker) = focus
            .clone()
            .downcast::<crate::widgets::MonthPicker>()
            .ok()
            .or_else(|| {
                focus
                    .ancestor(crate::widgets::MonthPicker::static_type())
                    .and_downcast::<crate::widgets::MonthPicker>()
            })
        && picker.key_pressed(key, modifiers) == glib::Propagation::Stop
    {
        return glib::Propagation::Stop;
    }

    if let Some(focus) = GtkWindowExt::focus(window)
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
        return picker.key_pressed(key, modifiers);
    }

    glib::Propagation::Proceed
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
            page.set_editor_owner_visible(false);
        }
    });

    let weak_page = page.downgrade();
    window.connect_map(move |window| {
        if let Some(page) = weak_page.upgrade() {
            sync_popup_visibility(window, &page);
            page.set_editor_owner_visible(true);
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
    fn shortcuts_help_opens_from_toolbar_and_action_and_closes() {
        gtk::init().unwrap();
        gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.hrniels.Eventix.ShortcutsTest")
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let (window, page) = Window::test_new(&app);
        window.present();
        settle();

        let toolbar = page.first_child().unwrap();
        let sync = toolbar.last_child().unwrap();
        let help = sync
            .prev_sibling()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        assert_eq!(help.action_name().as_deref(), Some("win.show-help-overlay"));
        assert_eq!(help.tooltip_text().as_deref(), Some("Shortcuts"));
        assert!(help.prev_sibling().is_some());
        assert_eq!(
            app.accels_for_action("win.show-help-overlay"),
            ["<Control>question"]
        );

        help.emit_clicked();
        settle();
        let overlay = window.imp().shortcuts.borrow().clone().unwrap();
        assert!(overlay.is_visible());
        assert!(overlay.is_modal());
        assert_eq!(overlay.transient_for().as_ref(), Some(window.upcast_ref()));
        assert_eq!(overlay.title().as_deref(), Some("Shortcuts"));
        overlay.close();
        settle();
        assert!(!overlay.is_visible());

        gio::prelude::ActionGroupExt::activate_action(&window, "show-help-overlay", None);
        settle();
        assert_eq!(window.imp().shortcuts.borrow().as_ref(), Some(&overlay));
        assert!(overlay.is_visible());
        let keys = overlay
            .observe_controllers()
            .snapshot()
            .into_iter()
            .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .unwrap();
        assert!(keys.emit_by_name::<bool>(
            "key-pressed",
            &[
                &gtk::gdk::Key::Escape,
                &0u32,
                &gtk::gdk::ModifierType::empty(),
            ]
        ));
        assert!(!overlay.is_visible());
        help.emit_clicked();
        assert!(overlay.is_visible());
        window.destroy();
        assert!(!overlay.is_visible());
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
    fn application_shortcuts_leave_text_inputs_to_their_default_bindings() {
        use std::rc::Rc;

        gtk::init().unwrap();
        gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.hrniels.Eventix.InputShortcutsTest")
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let (window, page) = Window::test_new(&app);
        let entry = gtk::Entry::new();
        let password = gtk::PasswordEntry::new();
        let text = gtk::TextView::new();
        page.append(&entry);
        page.append(&password);
        page.append(&text);
        window.present();
        settle();

        let shortcuts = [
            (gtk::gdk::Key::a, "quick-add-task"),
            (gtk::gdk::Key::e, "new-event"),
            (gtk::gdk::Key::f, "search"),
            (gtk::gdk::Key::t, "new-task"),
            (gtk::gdk::Key::w, "week-view"),
            (gtk::gdk::Key::m, "month-view"),
            (gtk::gdk::Key::l, "list-view"),
        ];
        let activated = Rc::new(RefCell::new(Vec::new()));
        for (_, name) in shortcuts {
            assert!(app.accels_for_action(&format!("win.{name}")).is_empty());
            let activated = activated.clone();
            window
                .lookup_action(name)
                .unwrap()
                .downcast::<gio::SimpleAction>()
                .unwrap()
                .connect_activate(move |_, _| activated.borrow_mut().push(name));
        }

        for input in [
            entry.upcast_ref::<gtk::Widget>(),
            password.upcast_ref(),
            text.upcast_ref(),
            page.imp().search.upcast_ref(),
        ] {
            assert!(input.grab_focus());
            for (key, _) in shortcuts {
                assert_eq!(
                    window.handle_key_pressed(key, gtk::gdk::ModifierType::CONTROL_MASK),
                    glib::Propagation::Proceed,
                );
            }
            assert!(activated.borrow().is_empty());
        }

        // Clipboard, undo, and word-editing keys are not application shortcuts either.
        for key in [
            gtk::gdk::Key::c,
            gtk::gdk::Key::x,
            gtk::gdk::Key::v,
            gtk::gdk::Key::z,
            gtk::gdk::Key::Left,
            gtk::gdk::Key::BackSpace,
        ] {
            assert_eq!(
                window.handle_key_pressed(key, gtk::gdk::ModifierType::CONTROL_MASK),
                glib::Propagation::Proceed,
            );
        }

        for (key, name) in shortcuts {
            GtkWindowExt::set_focus(&window, None::<&gtk::Widget>);
            assert_eq!(
                window.handle_key_pressed(key, gtk::gdk::ModifierType::CONTROL_MASK),
                glib::Propagation::Stop,
            );
            assert_eq!(activated.borrow_mut().pop(), Some(name));
            assert!(activated.borrow().is_empty());
        }
        window.destroy();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn search_shortcuts_work_in_search_and_cards_but_not_other_inputs_or_views() {
        use std::rc::Rc;

        gtk::init().unwrap();
        gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.hrniels.Eventix.SearchShortcutsTest")
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let (window, page) = Window::test_new(&app);
        let entry = gtk::Entry::new();
        let card = crate::view::card::EventCard::new();
        page.append(&entry);
        page.append(&card);
        window.present();
        settle();

        let activated = Rc::new(RefCell::new(Vec::new()));
        for name in ["find-next", "find-previous"] {
            let activated = activated.clone();
            window
                .lookup_action(name)
                .unwrap()
                .downcast::<gio::SimpleAction>()
                .unwrap()
                .connect_activate(move |_, _| activated.borrow_mut().push(name));
        }
        let control = gtk::gdk::ModifierType::CONTROL_MASK;
        let previous = control | gtk::gdk::ModifierType::SHIFT_MASK;
        for monthly in [false, true] {
            if monthly {
                page.switch_to_month()
            } else {
                page.switch_to_week()
            }
            for input in [
                page.imp().search.upcast_ref::<gtk::Widget>(),
                card.upcast_ref(),
            ] {
                assert!(input.grab_focus());
                for (key, modifiers, action) in [
                    (gtk::gdk::Key::g, control, "find-next"),
                    (gtk::gdk::Key::G, previous, "find-previous"),
                ] {
                    assert_eq!(
                        window.handle_key_pressed(key, modifiers),
                        glib::Propagation::Stop
                    );
                    assert_eq!(activated.borrow_mut().pop(), Some(action));
                }
            }
            assert!(entry.grab_focus());
            for modifiers in [control, previous] {
                assert_eq!(
                    window.handle_key_pressed(gtk::gdk::Key::g, modifiers),
                    glib::Propagation::Proceed
                );
            }
            assert!(activated.borrow().is_empty());
        }
        page.switch_to_list();
        card.grab_focus();
        for modifiers in [
            control,
            previous,
            control | gtk::gdk::ModifierType::ALT_MASK,
        ] {
            assert_eq!(
                window.handle_key_pressed(gtk::gdk::Key::g, modifiers),
                glib::Propagation::Proceed
            );
        }
        assert!(activated.borrow().is_empty());

        page.switch_to_week();
        card.grab_focus();
        assert_eq!(
            window
                .handle_key_pressed(gtk::gdk::Key::g, control | gtk::gdk::ModifierType::ALT_MASK,),
            glib::Propagation::Proceed,
        );
        page.imp().calendars_button.emit_clicked();
        assert!(page.management_open());
        for modifiers in [control, previous] {
            assert_eq!(
                window.handle_key_pressed(gtk::gdk::Key::g, modifiers),
                glib::Propagation::Proceed,
            );
        }
        assert!(activated.borrow().is_empty());
        window.destroy();
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn escape_defocuses_navigation_inputs_without_clearing_their_text() {
        gtk::init().unwrap();
        gio::resources_register_include!("eventix.gresource").unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.hrniels.Eventix.InputEscapeTest")
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let (window, page) = Window::test_new(&app);
        let quick = page.imp().sidebar.quick_task();
        quick.widget.set_visible(true);
        window.present();
        settle();

        for monthly in [false, true] {
            if monthly {
                page.switch_to_month();
            } else {
                page.switch_to_week();
            }
            for entry in [&*page.imp().search, &quick.summary] {
                entry.set_text("Unfinished draft");
                assert!(entry.grab_focus());
                settle();
                for key in [gtk::gdk::Key::Left, gtk::gdk::Key::Right] {
                    assert_eq!(
                        window.handle_key_pressed(key, gtk::gdk::ModifierType::empty()),
                        glib::Propagation::Proceed,
                    );
                }
                assert_eq!(
                    window.handle_key_pressed(
                        gtk::gdk::Key::Escape,
                        gtk::gdk::ModifierType::empty(),
                    ),
                    glib::Propagation::Stop,
                );
                settle();
                assert!(GtkWindowExt::focus(&window).is_none());
                assert_eq!(entry.text(), "Unfinished draft");
                for key in [gtk::gdk::Key::Left, gtk::gdk::Key::Right] {
                    assert_eq!(
                        window.handle_key_pressed(key, gtk::gdk::ModifierType::empty()),
                        glib::Propagation::Stop,
                    );
                }
            }
        }

        page.switch_to_list();
        page.focus_search();
        settle();
        let entry = GtkWindowExt::focus(&window)
            .unwrap()
            .ancestor(gtk::Entry::static_type())
            .and_downcast::<gtk::Entry>()
            .unwrap();
        entry.set_text("Unfinished search");
        assert_eq!(
            window.handle_key_pressed(gtk::gdk::Key::Escape, gtk::gdk::ModifierType::empty()),
            glib::Propagation::Proceed,
        );
        let keys = entry
            .observe_controllers()
            .snapshot()
            .into_iter()
            .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .unwrap();
        assert!(keys.emit_by_name::<bool>(
            "key-pressed",
            &[
                &gtk::gdk::Key::Escape,
                &0u32,
                &gtk::gdk::ModifierType::empty(),
            ]
        ));
        settle();
        assert!(GtkWindowExt::focus(&window).is_none());
        assert_eq!(entry.text(), "Unfinished search");
        page.focus_search();
        assert!(GtkWindowExt::focus(&window).is_some_and(|focus| focus.is_ancestor(&entry)));
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
