// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use gtk::{gdk, prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::model::calendars::CalendarVisibility;

/// Keeps header visibility controls synchronized without replacing focused buttons on a toggle.
#[derive(Default)]
pub(super) struct PageCalendars {
    choices: RefCell<Vec<(CalendarVisibility, gtk::ToggleButton)>>,
}

impl PageCalendars {
    pub(super) fn update(&self, page: &CalendarPage, calendars: Vec<CalendarVisibility>) {
        let mut choices = self.choices.borrow_mut();
        if choices.len() == calendars.len()
            && choices.iter().zip(&calendars).all(|((old, _), new)| {
                old.id == new.id && old.name == new.name && old.background == new.background
            })
        {
            for ((old, button), new) in choices.iter_mut().zip(calendars) {
                button.set_active(new.enabled);
                *old = new;
            }
            return;
        }

        let container = &page.imp().calendars;
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        choices.clear();
        for calendar in calendars {
            let dot = gtk::DrawingArea::new();
            dot.set_size_request(22, 22);
            let button = gtk::ToggleButton::builder()
                .child(&dot)
                .active(calendar.enabled)
                .tooltip_text(&calendar.name)
                .build();
            draw_circle(&dot, &button, &calendar.background);
            button.add_css_class("ev_calendar_toggle");
            button.update_property(&[gtk::accessible::Property::Label(&calendar.name)]);
            let weak = page.downgrade();
            let id = calendar.id.clone();
            button.connect_clicked(move |_| {
                if let Some(page) = weak.upgrade()
                    && let Some(session) = page.imp().session.current.borrow().as_ref()
                {
                    page.dismiss_popovers();
                    session.toggle_calendar(id.clone());
                }
            });
            container.append(&button);
            choices.push((calendar, button));
        }
    }
}

fn draw_circle(dot: &gtk::DrawingArea, button: &gtk::ToggleButton, background: &str) {
    let color = gdk::RGBA::parse(background).unwrap_or(gdk::RGBA::TRANSPARENT);
    let weak = button.downgrade();
    dot.set_draw_func(move |_, cr, width, height| {
        let Some(button) = weak.upgrade() else { return };
        if button.is_active() {
            cr.set_source_rgba(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                color.alpha().into(),
            );
        } else {
            cr.set_source_rgb(0.65, 0.65, 0.65);
        }
        cr.arc(
            f64::from(width) / 2.0,
            f64::from(height) / 2.0,
            f64::from(width.min(height)) / 2.0 - 1.0,
            0.0,
            std::f64::consts::TAU,
        );
        let _ = cr.fill_preserve();
        if button.state_flags().contains(gtk::StateFlags::PRELIGHT) {
            cr.set_source_rgb(0.0, 0.0, 0.0);
            cr.set_line_width(1.0);
            let _ = cr.stroke();
        }
    });
    let weak = dot.downgrade();
    button.connect_state_flags_changed(move |_, _| {
        if let Some(dot) = weak.upgrade() {
            dot.queue_draw();
        }
    });
}

#[cfg(test)]
mod tests {
    use gtk::{prelude::*, subclass::prelude::*};

    use super::{CalendarPage, CalendarVisibility};

    #[test]
    #[ignore = "requires a GTK display"]
    fn header_calendar_toggles_load_visibility_and_preserve_buttons_on_refresh() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("eventix.gresource").unwrap();
        crate::style::install(&gtk::gdk::Display::default().unwrap());
        let page: CalendarPage = gtk::glib::Object::new();
        let imp = page.imp();
        let mut calendars = vec![
            CalendarVisibility {
                id: "red".into(),
                name: "Same name".into(),
                background: "#ff0000".into(),
                enabled: false,
            },
            CalendarVisibility {
                id: "blue".into(),
                name: "Same name".into(),
                background: "#0000ff".into(),
                enabled: true,
            },
        ];
        imp.calendar_toggles.update(&page, calendars.clone());
        let first = imp
            .calendars
            .first_child()
            .and_downcast::<gtk::ToggleButton>()
            .unwrap();
        let second = first
            .next_sibling()
            .and_downcast::<gtk::ToggleButton>()
            .unwrap();
        assert!(!first.is_active());
        assert!(second.is_active());
        assert_eq!(first.tooltip_text().as_deref(), Some("Same name"));
        assert!(second.next_sibling().is_none());

        let window = gtk::Window::builder()
            .default_width(1200)
            .default_height(700)
            .child(&page)
            .build();
        window.present();
        for _ in 0..10 {
            while gtk::glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(imp.calendars_button.label().as_deref(), Some("Calendars:"));
        let label_bounds = imp.calendars_button.compute_bounds(&page).unwrap();
        let circle_bounds = first.compute_bounds(&page).unwrap();
        let view_bounds = imp.week_view.compute_bounds(&page).unwrap();
        assert!(label_bounds.x() + label_bounds.width() <= circle_bounds.x());
        assert!(circle_bounds.width() >= 22.0);
        assert!(circle_bounds.x() + circle_bounds.width() < view_bounds.x());

        calendars[0].enabled = true;
        calendars[1].enabled = false;
        imp.calendar_toggles.update(&page, calendars.clone());
        assert_eq!(imp.calendars.first_child(), Some(first.clone().upcast()));
        assert!(first.is_active());
        assert!(!second.is_active());

        calendars.remove(0);
        calendars[0].name = "Renamed".into();
        imp.calendar_toggles.update(&page, calendars);
        let only = imp.calendars.first_child().unwrap();
        assert_eq!(only.tooltip_text().as_deref(), Some("Renamed"));
        assert!(only.next_sibling().is_none());
        imp.calendar_toggles.update(&page, vec![]);
        assert!(imp.calendars.first_child().is_none());
        window.close();
    }
}
