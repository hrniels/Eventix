// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use eventix_locale::{Locale, TimeFlags};
use eventix_view::Event;
use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::style;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/hrniels/Eventix/gtk/view/weekly/card.ui")]
    pub struct EventCard {
        #[template_child]
        pub text: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub indicators: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub recurrence: gtk::TemplateChild<gtk::Image>,
        #[template_child]
        pub exception: gtk::TemplateChild<gtk::Image>,
        #[template_child]
        pub alarm: gtk::TemplateChild<gtk::Image>,
        pub color_class: RefCell<String>,
        pub event: RefCell<Option<Event>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EventCard {
        const NAME: &'static str = "EventixEventCard";
        type Type = super::EventCard;
        type ParentType = gtk::Button;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for EventCard {
        fn dispose(&self) {
            self.obj().set_child(None::<&gtk::Widget>);
            self.dispose_template();
        }
    }
    impl WidgetImpl for EventCard {}
    impl ButtonImpl for EventCard {}
}

glib::wrapper! {
    pub struct EventCard(ObjectSubclass<imp::EventCard>)
        @extends gtk::Button, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl EventCard {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn event(&self) -> Option<Event> {
        self.imp().event.borrow().clone()
    }

    pub fn set_event(&self, event: &Event, locale: &dyn Locale, strip: bool) {
        let summary = if event.summary.is_empty() {
            locale.translate("Untitled event")
        } else {
            &event.summary
        };
        let time = if event.all_day {
            locale.translate("All day").to_owned()
        } else {
            format!(
                "{}{}–{}{}",
                if event.start.date_naive() < event.id.date {
                    "← "
                } else {
                    ""
                },
                locale.fmt_time(&event.start, TimeFlags::Short),
                locale.fmt_time(&event.end, TimeFlags::Short),
                if event.end.date_naive() > event.id.date {
                    " →"
                } else {
                    ""
                }
            )
        };
        let text = if strip {
            summary.to_owned()
        } else if event.minute_end.saturating_sub(event.minute_start) < 38 {
            format!("{summary} · {time}")
        } else {
            format!("{summary}\n{time}")
        };
        self.imp().text.set_text(&text);
        self.imp().recurrence.set_visible(event.recurrent);
        self.imp().exception.set_visible(event.exception);
        self.imp().alarm.set_visible(event.alarm);
        self.imp()
            .indicators
            .set_visible(event.recurrent || event.exception || event.alarm);
        if event.cancelled {
            self.add_css_class("ev_cancelled");
        } else {
            self.remove_css_class("ev_cancelled");
        }
        let color_class = style::calendar_class(&event.background, &event.foreground);
        let old = self.imp().color_class.replace(color_class.clone());
        if old != color_class {
            if !old.is_empty() {
                self.remove_css_class(&old);
            }
            self.add_css_class(&color_class);
        }
        self.imp().event.replace(Some(event.clone()));
    }
}
