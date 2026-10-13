// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{glib, prelude::*, subclass::prelude::*};

use super::{all_day_strip::AllDayStrip, timed_grid::TimedGrid};
use crate::view::geometry::GUTTER;

pub(crate) mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/hrniels/Eventix/gtk/view/weekly/content.ui")]
    pub struct WeekContent {
        #[template_child]
        pub headers: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub gutter: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub monday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub tuesday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub wednesday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub thursday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub friday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub saturday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub sunday: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub scroll: gtk::TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub timed: gtk::TemplateChild<TimedGrid>,
        #[template_child]
        pub all_day: gtk::TemplateChild<AllDayStrip>,
        #[template_child]
        pub all_day_label: gtk::TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WeekContent {
        const NAME: &'static str = "EventixWeekContent";
        type Type = super::WeekContent;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            TimedGrid::ensure_type();
            AllDayStrip::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for WeekContent {
        fn constructed(&self) {
            self.parent_constructed();
            self.gutter.set_size_request(GUTTER, -1);
            self.headers.set_margin_end(
                self.scroll
                    .vscrollbar()
                    .measure(gtk::Orientation::Horizontal, -1)
                    .1,
            );
        }

        fn dispose(&self) {
            self.dispose_template();
        }
    }

    impl WidgetImpl for WeekContent {}
    impl BoxImpl for WeekContent {}
}

glib::wrapper! {
    pub struct WeekContent(ObjectSubclass<imp::WeekContent>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}
