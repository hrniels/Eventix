// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_locale::{DateFlags, Locale};
use eventix_view::Event;
use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/hrniels/Eventix/gtk/event_details.ui")]
    pub struct EventDetails {
        #[template_child]
        pub summary: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub calendar: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub start: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub end: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub timezone: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub location: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub description: gtk::TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EventDetails {
        const NAME: &'static str = "EventixEventDetails";
        type Type = super::EventDetails;
        type ParentType = gtk::Popover;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for EventDetails {
        fn dispose(&self) {
            self.dispose_template();
        }
    }
    impl WidgetImpl for EventDetails {}
    impl PopoverImpl for EventDetails {}
}

glib::wrapper! {
    pub struct EventDetails(ObjectSubclass<imp::EventDetails>)
        @extends gtk::Popover, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
            gtk::Native, gtk::ShortcutManager;
}

impl EventDetails {
    pub fn new(event: &Event, locale: &dyn Locale) -> Self {
        let obj: Self = glib::Object::new();
        let imp = obj.imp();
        let start = if event.all_day {
            locale.fmt_date(&event.start, DateFlags::NoToday)
        } else {
            locale.fmt_datetime(&event.start, DateFlags::NoToday)
        };
        let end = if event.all_day {
            locale.fmt_date(&event.end, DateFlags::NoToday)
        } else {
            locale.fmt_datetime(&event.end, DateFlags::NoToday)
        };
        let timezone = locale.timezone().to_string();
        let summary = if event.summary.is_empty() {
            locale.translate("Untitled event")
        } else {
            &event.summary
        };
        for (label, key, value) in [
            (&imp.summary, "Summary", summary),
            (&imp.calendar, "Calendar", &event.calendar),
            (&imp.start, "Start", &start),
            (&imp.end, "End", &end),
            (&imp.timezone, "Timezone", &timezone),
            (&imp.location, "Location", &event.location),
            (&imp.description, "Description", &event.description),
        ] {
            label.set_visible(!value.is_empty());
            label.set_text(&format!("{}: {value}", locale.translate(key)));
        }
        obj
    }
}
