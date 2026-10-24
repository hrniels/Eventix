// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::NaiveDate;
use gtk::{gio, glib, prelude::*, subclass::prelude::*};

use crate::view::{card::EventCard, details::EventDetails, page::CalendarPage};

mod imp {
    use super::*;

    resource_template! { "window.ui",
    pub struct Window {
        #[template_child]
        pub page: gtk::TemplateChild<CalendarPage>,
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

    impl WidgetImpl for Window {}

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

        if modifiers.contains(gtk::gdk::ModifierType::ALT_MASK)
            && !modifiers.intersects(
                gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SUPER_MASK,
            )
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

        let mut focused = gtk::prelude::GtkWindowExt::focus(self);
        while let Some(widget) = focused {
            if widget.has_css_class("ev_sidebar") {
                return glib::Propagation::Proceed;
            }

            focused = widget.parent();
        }

        self.imp()
            .page
            .focus_event(self.focused_event_card().as_ref(), forward);

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
        let action = gio::SimpleAction::new("new-event", None);
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(page) = page.upgrade() {
                page.new_event();
            }
        });
        self.add_action(&action);

        let action = gio::SimpleAction::new("new-task", None);
        let page = self.imp().page.downgrade();
        action.connect_activate(move |_, _| {
            if let Some(page) = page.upgrade() {
                page.new_task();
            }
        });
        self.add_action(&action);

        for (name, keys, days) in [
            ("previous", vec!["<Alt>Left"], Some(-1)),
            ("next", vec!["<Alt>Right"], Some(1)),
            ("today", vec!["<Alt>Home", "period"], None),
            ("reload", vec!["<Primary>r", "F5"], None),
            ("week-view", vec!["<Alt>w"], None),
            ("month-view", vec!["<Alt>m"], None),
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
                    } else {
                        page.show_content(None);
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
    // Nested popup grabs can temporarily deactivate GTK's window while its native
    // toplevel remains focused. Switching applications must hide, not dismiss, its popup.
    let focused = window.is_active()
        || window
            .surface()
            .and_downcast::<gtk::gdk::Toplevel>()
            .is_some_and(|surface| surface.state().contains(gtk::gdk::ToplevelState::FOCUSED));

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
