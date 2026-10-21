// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use eventix_ical::objects::RangeEdge;
use eventix_locale::{Locale, TimeFlags};
use eventix_view::details::Response;
use eventix_view::{AllDaySpan, Event, MonthEvent, TimedEvent};
use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::actions::items::move_event::{self, DraggedEvent};
use crate::style;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/hrniels/Eventix/gtk/view/card.ui")]
    pub struct EventCard {
        #[template_child]
        pub response: gtk::TemplateChild<gtk::Box>,
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
        pub(super) drag: RefCell<Option<DraggedEvent>>,
        pub resize: RefCell<Option<TimedEvent>>,
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
        fn constructed(&self) {
            self.parent_constructed();
            move_event::install_source(&self.obj());
            crate::actions::items::resize_event::install(&self.obj());
        }

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

    pub fn set_span(&self, span: &AllDaySpan, locale: &dyn Locale) {
        self.imp().drag.take();
        self.imp().resize.take();
        let summary = self.summary(&span.event, locale);
        let text = format!(
            "{}{}{}",
            if span.continues_before { "← " } else { "" },
            summary,
            if span.continues_after { " →" } else { "" },
        );
        self.set_event(&span.event, &text);
    }

    pub fn set_timed(&self, segment: &TimedEvent, locale: &dyn Locale) {
        self.imp().drag.replace(DraggedEvent::new(segment));
        self.imp().resize.replace(Some(segment.clone()));
        let event = &segment.event;
        let summary = self.summary(event, locale);
        let time = format!(
            "{}{}–{}{}",
            if event.start.date_naive() < segment.date {
                "← "
            } else {
                ""
            },
            locale.fmt_time(&event.start, TimeFlags::Short),
            locale.fmt_time(&event.end, TimeFlags::Short),
            if event.end.date_naive() > segment.date {
                " →"
            } else {
                ""
            },
        );
        let text = if segment.minute_end.saturating_sub(segment.minute_start) < 38 {
            format!("{summary} · {time}")
        } else {
            format!("{summary}\n{time}")
        };
        self.set_event(event, &text);
    }

    pub fn set_monthly(&self, segment: &MonthEvent, locale: &dyn Locale) {
        self.imp().drag.take();
        self.imp().resize.take();
        let event = &segment.event;
        self.imp()
            .text
            .set_xalign(if segment.full_day { 0.5 } else { 0.0 });
        let summary = self.summary(event, locale);
        let text = if segment.full_day {
            summary.to_string()
        } else {
            let starts = event.start.date_naive() == segment.date;
            let logical_end = if event.end.time() == chrono::NaiveTime::MIN {
                event.end.date_naive().pred_opt().unwrap_or(segment.date)
            } else {
                event.end.date_naive()
            };
            let ends = logical_end == segment.date;
            let time = match (starts, ends) {
                (true, false) => format!("→ {}", locale.fmt_time(&event.start, TimeFlags::Short)),
                (false, true) => format!("← {}", locale.fmt_time(&event.end, TimeFlags::Short)),
                _ => format!(
                    "{}–{}",
                    locale.fmt_time(&event.start, TimeFlags::Short),
                    locale.fmt_time(&event.end, TimeFlags::Short)
                ),
            };
            format!("{time} {summary}")
        };
        self.set_event(event, &text);
    }

    fn summary<'a>(&self, event: &'a Event, locale: &'a dyn Locale) -> &'a str {
        if event.summary.is_empty() {
            locale.translate("Untitled event")
        } else {
            &event.summary
        }
    }

    pub(crate) fn drag_payload(&self, y: f64) -> Option<DraggedEvent> {
        if self.resize_edge(y).is_some() {
            return None;
        }
        let mut payload = self.imp().drag.borrow().clone()?;
        payload.offset_minutes += y.round() as i64;
        Some(payload)
    }

    pub(crate) fn resize_segment(&self) -> Option<TimedEvent> {
        self.imp().resize.borrow().clone()
    }

    pub(crate) fn resize_edge(&self, y: f64) -> Option<RangeEdge> {
        let segment = self.imp().resize.borrow();
        let segment = segment.as_ref()?;
        if !segment.event.details.actions.edit {
            return None;
        }
        if y < 5.0 && segment.event.start.date_naive() == segment.date {
            Some(RangeEdge::Start)
        } else if y >= f64::from(self.height()) - 5.0
            && segment.event.end.naive_local() <= segment.date.succ_opt()?.and_hms_opt(0, 0, 0)?
        {
            Some(RangeEdge::End)
        } else {
            None
        }
    }

    fn set_event(&self, event: &Event, text: &str) {
        self.imp().text.set_text(text);
        self.set_tooltip_text(Some(text));
        let actions = &event.details.actions;
        let response = actions.occurrence_response.or(actions.series_response);
        self.imp().response.set_visible(response.is_some());
        self.imp().response.set_css_classes(&[
            "ev_response_bar",
            match response {
                Some(Response::Accepted) => "ev_response_accepted",
                Some(Response::Tentative) => "ev_response_tentative",
                Some(Response::Declined) => "ev_response_declined",
                _ => "ev_response_pending",
            },
        ]);
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
