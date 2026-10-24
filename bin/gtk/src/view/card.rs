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

use super::indicators::EventIndicators;

mod imp {
    use super::*;

    resource_template! { "view/card.ui",
    pub struct EventCard {
        #[template_child]
        pub content: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub response: gtk::TemplateChild<gtk::Box>,
        #[template_child]
        pub text: gtk::TemplateChild<gtk::Label>,
        #[template_child]
        pub indicators: gtk::TemplateChild<EventIndicators>,
        pub color_class: RefCell<String>,
        pub event: RefCell<Option<Event>>,
        pub(super) drag: RefCell<Option<DraggedEvent>>,
        pub resize: RefCell<Option<TimedEvent>>,
    }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EventCard {
        const NAME: &'static str = "EventixEventCard";
        type Type = super::EventCard;
        type ParentType = gtk::Button;

        fn class_init(klass: &mut Self::Class) {
            EventIndicators::static_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for EventCard {
        fn constructed(&self) {
            self.parent_constructed();

            // Allocate the content ourselves so indicators adapt before the box lays them out.
            self.obj().set_layout_manager(None::<gtk::LayoutManager>);
            move_event::install_source(&self.obj());
            crate::actions::items::resize_event::install(&self.obj());
        }

        fn dispose(&self) {
            self.obj().set_child(None::<&gtk::Widget>);
            self.dispose_template();
        }
    }

    impl WidgetImpl for EventCard {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let (_, natural, _, _) = self.content.measure(orientation, for_size);

            // Calendar geometry determines card sizes, even below the content's minimum size.
            (0, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.obj().fit_indicators(width, height);
            self.content.allocate(width, height, baseline, None);
        }
    }

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

        self.set_event(&span.event, &text, locale);
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

        self.set_event(event, &text, locale);
    }

    pub fn set_monthly(&self, segment: &MonthEvent, locale: &dyn Locale) {
        self.imp()
            .drag
            .replace(DraggedEvent::for_month(&segment.event));
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

        self.set_event(event, &text, locale);
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
        if self.imp().resize.borrow().is_some() {
            payload.offset_minutes += y.round() as i64;
        }

        Some(payload)
    }

    pub(crate) fn resize_segment(&self) -> Option<TimedEvent> {
        self.imp().resize.borrow().clone()
    }

    pub(crate) fn enable_date_drag(&self) {
        self.imp()
            .drag
            .replace(self.event().as_ref().and_then(DraggedEvent::for_month));
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

    fn set_event(&self, event: &Event, text: &str, locale: &dyn Locale) {
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

        self.imp()
            .indicators
            .set_state(event.recurrent, event.exception, event.alarm, locale);

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

    /// Keeps only indicators that fit alongside the label in the card's content area.
    fn fit_indicators(&self, width: i32, height: i32) {
        let imp = self.imp();
        let spacing = imp.content.spacing();
        let mut available = width - imp.text.measure(gtk::Orientation::Horizontal, -1).0;
        if imp.response.is_visible() {
            available -= imp.response.measure(gtk::Orientation::Horizontal, -1).1 + spacing;
        }

        available -= spacing + imp.indicators.margin_start() + imp.indicators.margin_end();
        let available_height =
            height - imp.indicators.margin_top() - imp.indicators.margin_bottom();

        imp.indicators.fit(available, available_height);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eventix_view::{
        OccurrenceId,
        details::{DetailActions, Details},
    };

    use gtk::{glib, prelude::*, subclass::prelude::*};

    use super::{Event, EventCard, Response};

    #[test]
    #[ignore = "requires a GTK display"]
    fn indicators_fit_and_reappear_when_cards_resize() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let start = chrono::DateTime::from_timestamp(0, 0)
            .unwrap()
            .with_timezone(&chrono_tz::UTC);
        let mut event = Event {
            id: OccurrenceId {
                calendar: "test".into(),
                uid: "test".into(),
                recurrence: String::new(),
            },
            summary: "Event with indicators".into(),
            calendar: "Test".into(),
            foreground: "#ffffff".into(),
            background: "#336699".into(),
            start,
            end: start + chrono::Duration::hours(1),
            all_day: false,
            cancelled: false,
            recurrent: true,
            exception: true,
            alarm: true,
            details: Arc::new(Details {
                date_label: "Date",
                date: String::new(),
                source_date: None,
                location: String::new(),
                description: String::new(),
                status: None,
                recurrence: None,
                reminders: None,
                organizer: None,
                sent_by: None,
                attendees: vec![],
                actions: DetailActions {
                    occurrence_response: Some(Response::Accepted),
                    ..Default::default()
                },
            }),
        };
        let card = EventCard::new();
        let locale = eventix_locale::default();
        card.set_event(&event, &event.summary, &*locale);
        let window = gtk::Window::builder().child(&card).build();
        window.present();
        while glib::MainContext::default().iteration(false) {}

        let icons: Vec<gtk::Image> = card
            .imp()
            .indicators
            .observe_children()
            .snapshot()
            .into_iter()
            .map(|child| child.downcast().unwrap())
            .collect();
        let mut saw_partial = false;
        for width in (10..=160).rev() {
            card.allocate(width, 30, -1, None);
            let visible = icons.iter().filter(|icon| icon.is_visible()).count();
            saw_partial |= visible > 0 && visible < icons.len();
            for icon in icons.iter().filter(|icon| icon.is_visible()) {
                let bounds = icon.compute_bounds(&*card.imp().content).unwrap();
                assert!(bounds.x() >= 0.0 && bounds.y() >= 0.0);
                assert!(
                    bounds.x() + bounds.width() <= card.imp().content.width() as f32,
                    "requested {width}, card {}, content {}, bounds {bounds:?}",
                    card.width(),
                    card.imp().content.width(),
                );
                assert!(bounds.y() + bounds.height() <= card.imp().content.height() as f32);
            }
        }
        assert!(saw_partial);
        assert!(icons.iter().all(|icon| !icon.is_visible()));
        card.allocate(200, 10, -1, None);
        assert!(icons.iter().all(|icon| !icon.is_visible()));
        card.allocate(200, 30, -1, None);
        assert!(icons.iter().all(|icon| icon.is_visible()));

        event.recurrent = false;
        event.exception = false;
        card.set_event(&event, &event.summary, &*locale);
        card.allocate(200, 30, -1, None);
        assert!(!icons[0].is_visible());
        assert!(!icons[1].is_visible());
        assert!(icons[2].is_visible());

        card.set_event(&event, "Event\n09:00–10:00", &*locale);
        for width in (10..=160).rev() {
            card.allocate(width, 40, -1, None);
            if icons[2].is_visible() {
                let bounds = icons[2].compute_bounds(&*card.imp().content).unwrap();
                assert!(bounds.x() + bounds.width() <= card.imp().content.width() as f32);
            }
        }
        window.close();
    }
}
