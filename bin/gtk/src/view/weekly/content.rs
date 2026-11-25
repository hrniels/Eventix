// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{glib, prelude::*, subclass::prelude::*};

use super::{all_day_strip::AllDayStrip, timed_grid::TimedGrid};
use crate::view::geometry::GUTTER;

pub(crate) mod imp {
    use super::*;

    resource_template! { "view/weekly/content.ui",
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
        pub monday_time_change: gtk::TemplateChild<gtk::Image>,
        #[template_child]
        pub tuesday_time_change: gtk::TemplateChild<gtk::Image>,
        #[template_child]
        pub wednesday_time_change: gtk::TemplateChild<gtk::Image>,
        #[template_child]
        pub thursday_time_change: gtk::TemplateChild<gtk::Image>,
        #[template_child]
        pub friday_time_change: gtk::TemplateChild<gtk::Image>,
        #[template_child]
        pub saturday_time_change: gtk::TemplateChild<gtk::Image>,
        #[template_child]
        pub sunday_time_change: gtk::TemplateChild<gtk::Image>,
        #[template_child]
        pub all_day_scroll: gtk::TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub all_day_top_fade: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub all_day_bottom_fade: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub scroll: gtk::TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub timed: gtk::TemplateChild<TimedGrid>,
        #[template_child]
        pub all_day: gtk::TemplateChild<AllDayStrip>,
    }
    }

    impl WeekContent {
        pub fn time_change_icons(&self) -> [&gtk::Image; 7] {
            [
                &self.monday_time_change,
                &self.tuesday_time_change,
                &self.wednesday_time_change,
                &self.thursday_time_change,
                &self.friday_time_change,
                &self.saturday_time_change,
                &self.sunday_time_change,
            ]
        }
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

            // Week changes reset the timeline to a fixed time. Focus changes while repacking
            // cards must not scroll the viewport back to a previously selected card.
            for scroll in [&self.all_day_scroll, &self.scroll] {
                if let Some(viewport) = scroll.child().and_downcast::<gtk::Viewport>() {
                    viewport.set_scroll_to_focus(false);
                }
            }

            let top_fade = self.all_day_top_fade.downgrade();
            let bottom_fade = self.all_day_bottom_fade.downgrade();
            let all_day = self.all_day.downgrade();
            let update_viewport = move |adjustment: &gtk::Adjustment| {
                let at_top = adjustment.value() <= adjustment.lower();
                let at_bottom = adjustment.value() + adjustment.page_size() >= adjustment.upper();
                if let Some(fade) = top_fade.upgrade() {
                    fade.set_visible(!at_top);
                }
                if let Some(fade) = bottom_fade.upgrade() {
                    fade.set_visible(!at_bottom);
                }
                if let Some(strip) = all_day.upgrade() {
                    // The preview label is pinned to the viewport, not the content.
                    strip.queue_draw();
                }
            };
            let adjustment = self.all_day_scroll.vadjustment();
            adjustment.connect_changed(update_viewport.clone());
            adjustment.connect_value_changed(update_viewport);
        }

        fn dispose(&self) {
            self.dispose_template();
        }
    }

    impl WidgetImpl for WeekContent {
        fn focus(&self, direction: gtk::DirectionType) -> bool {
            match direction {
                gtk::DirectionType::TabForward | gtk::DirectionType::TabBackward => {
                    let order = self
                        .obj()
                        .focus_cards()
                        .into_iter()
                        .map(|card| card.upcast())
                        .collect::<Vec<_>>();
                    crate::view::focus::traverse(
                        self.obj().upcast_ref(),
                        &order,
                        direction == gtk::DirectionType::TabForward,
                    )
                }
                _ => false,
            }
        }
    }

    impl BoxImpl for WeekContent {}
}

glib::wrapper! {
    pub struct WeekContent(ObjectSubclass<imp::WeekContent>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl WeekContent {
    /// Orders cards by their first visible day, with strip events before timed events.
    pub(crate) fn focus_cards(&self) -> Vec<crate::view::card::EventCard> {
        let imp = self.imp();
        let mut cards = imp.all_day.focus_cards();
        cards.extend(imp.timed.focus_cards());
        cards.sort_by_key(|(day, minute, _)| (*day, *minute));
        cards.into_iter().map(|(_, _, card)| card).collect()
    }
}
