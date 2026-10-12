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
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = obj.downgrade();
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
        obj.add_controller(click);
        let weak = obj.downgrade();
        obj.connect_hide(move |_| {
            if let Some(window) = weak.upgrade() {
                window.imp().page.dismiss_popovers();
            }
        });
        let weak = obj.downgrade();
        obj.connect_is_active_notify(move |window| {
            if !window.is_active()
                && let Some(window) = weak.upgrade()
            {
                window.imp().page.dismiss_popovers();
            }
        });
        for (name, keys, days) in [
            ("previous", vec!["<Alt>Left"], Some(-7)),
            ("next", vec!["<Alt>Right"], Some(7)),
            ("today", vec!["<Alt>Home"], None),
            ("reload", vec!["<Primary>r", "F5"], None),
        ] {
            let action = gio::SimpleAction::new(name, None);
            let page = obj.imp().page.downgrade();
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
            obj.add_action(&action);
            app.set_accels_for_action(&format!("win.{name}"), &keys);
        }
        obj.imp().page.start(prefix, date);
        obj
    }
}
