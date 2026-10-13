// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::NaiveDate;
use gtk::{gio, glib, prelude::*, subclass::prelude::*};

use crate::{event_card::EventCard, event_details::EventDetails, week_page::WeekPage};

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/hrniels/Eventix/gtk/window.ui")]
    pub struct Window {
        #[template_child]
        pub page: gtk::TemplateChild<WeekPage>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "EventixWindow";
        type Type = super::Window;
        type ParentType = gtk::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            WeekPage::ensure_type();
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
        let obj: Self = glib::Object::builder().property("application", app).build();
        obj.setup_click_controller();
        obj.setup_key_controller();
        obj.connect_popover_dismissal();
        obj.setup_actions(app);
        obj.imp().page.start(prefix, date);
        obj
    }

    fn setup_click_controller(&self) {
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.downgrade();
        click.connect_pressed(move |_, _, x, y| {
            let Some(window) = weak.upgrade() else { return };
            let inside_card_or_popup =
                window
                    .pick(x, y, gtk::PickFlags::DEFAULT)
                    .is_some_and(|mut widget| {
                        loop {
                            if widget.downcast_ref::<EventCard>().is_some()
                                || widget.downcast_ref::<EventDetails>().is_some()
                            {
                                break true;
                            }
                            let Some(parent) = widget.parent() else {
                                break false;
                            };
                            widget = parent;
                        }
                    });
            if !inside_card_or_popup {
                window.imp().page.dismiss_popovers();
            }
        });
        self.add_controller(click);
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
                    self.imp().page.navigate(-7);
                    return glib::Propagation::Stop;
                }
                gtk::gdk::Key::Right => {
                    self.imp().page.navigate(7);
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
        let weak = self.downgrade();
        self.connect_hide(move |_| {
            if let Some(window) = weak.upgrade() {
                window.imp().page.dismiss_popovers();
            }
        });
        let weak = self.downgrade();
        self.connect_is_active_notify(move |window| {
            if !window.is_active()
                && let Some(window) = weak.upgrade()
            {
                window.imp().page.dismiss_popovers();
            }
        });
    }

    fn setup_actions(&self, app: &gtk::Application) {
        for (name, keys, days) in [
            ("previous", vec!["<Alt>Left"], Some(-7)),
            ("next", vec!["<Alt>Right"], Some(7)),
            ("today", vec!["<Alt>Home", "period"], None),
            ("reload", vec!["<Primary>r", "F5"], None),
        ] {
            let action = gio::SimpleAction::new(name, None);
            let page = self.imp().page.downgrade();
            action.connect_activate(move |_, _| {
                if let Some(page) = page.upgrade() {
                    if let Some(days) = days {
                        page.navigate(days);
                    } else if name == "reload" {
                        page.reload();
                    } else {
                        page.request(None, false);
                    }
                }
            });
            self.add_action(&action);
            app.set_accels_for_action(&format!("win.{name}"), &keys);
        }
    }
}
