// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use gtk::{prelude::*, subclass::prelude::*};

use super::CalendarPage;
use crate::model::calendars::CalendarVisibility;
use crate::view::calendar_selector::draw_toggle_circle;

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
                update_sync_error(button, &new);
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
            let content = gtk::Overlay::new();
            content.set_child(Some(&dot));
            let warning = gtk::DrawingArea::new();
            warning.set_can_target(false);
            content.add_overlay(&warning);
            let button = gtk::ToggleButton::builder()
                .child(&content)
                .active(calendar.enabled)
                .build();
            update_sync_error(&button, &calendar);
            draw_toggle_circle(&dot, &button, &calendar.background);
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

fn update_sync_error(button: &gtk::ToggleButton, calendar: &CalendarVisibility) {
    let tooltip = match &calendar.sync_error {
        Some(error) => format!("{}\n{}", calendar.name, error),
        None => calendar.name.clone(),
    };
    button.set_tooltip_text(Some(&tooltip));
    if let Some(content) = button.child().and_downcast::<gtk::Overlay>()
        && let Some(warning) = content.last_child().and_downcast::<gtk::DrawingArea>()
    {
        warning.set_visible(calendar.sync_error.is_some());
        let color = sync_error_color(&calendar.background, calendar.enabled);
        warning.set_draw_func(move |_, cr, width, height| {
            cr.set_source_rgb(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
            );
            let x = f64::from(width) / 2.0;
            let y = f64::from(height) / 2.0;
            cr.rectangle(x - 1.0, y - 6.0, 2.0, 8.0);
            cr.rectangle(x - 1.0, y + 4.0, 2.0, 2.0);
            let _ = cr.fill();
        });
    }
}

/// Chooses a contrasting foreground, preferring white on darker mid-tone circles.
fn sync_error_color(background: &str, enabled: bool) -> gtk::gdk::RGBA {
    let background = if enabled {
        gtk::gdk::RGBA::parse(background).unwrap_or(gtk::gdk::RGBA::BLACK)
    } else {
        gtk::gdk::RGBA::new(0.65, 0.65, 0.65, 1.0)
    };
    let linear = |channel: f32| {
        let channel = f64::from(channel);
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(background.red())
        + 0.7152 * linear(background.green())
        + 0.0722 * linear(background.blue());
    // Favor white beyond the equal-contrast point so saturated reds also use a light mark.
    if luminance < 0.3 {
        gtk::gdk::RGBA::WHITE
    } else {
        gtk::gdk::RGBA::BLACK
    }
}

#[cfg(test)]
mod tests {
    use gtk::{prelude::*, subclass::prelude::*};

    use super::{CalendarPage, CalendarVisibility, sync_error_color};

    #[test]
    fn sync_error_mark_contrasts_with_calendar_colors_and_inactive_gray() {
        for background in ["#000000", "#0000ff", "#333333", "#ff0000", "#ed3346"] {
            assert_eq!(sync_error_color(background, true), gtk::gdk::RGBA::WHITE);
        }
        for background in ["#ffffff", "#ffff00", "#bbbbbb"] {
            assert_eq!(sync_error_color(background, true), gtk::gdk::RGBA::BLACK);
        }
        for background in ["#000000", "#ffffff", "#0000ff"] {
            assert_eq!(sync_error_color(background, false), gtk::gdk::RGBA::BLACK);
        }
    }

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
                sync_error: None,
            },
            CalendarVisibility {
                id: "blue".into(),
                name: "Same name".into(),
                background: "#0000ff".into(),
                enabled: true,
                sync_error: None,
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
        let view_bounds = imp.list_view.compute_bounds(&page).unwrap();
        let search_bounds = imp.search.compute_bounds(&page).unwrap();
        let last_circle_bounds = second.compute_bounds(&page).unwrap();
        let reload_bounds = imp.reload.compute_bounds(&page).unwrap();
        assert!(view_bounds.x() + view_bounds.width() < search_bounds.x());
        assert!(search_bounds.x() + search_bounds.width() < label_bounds.x());
        assert!(label_bounds.x() + label_bounds.width() <= circle_bounds.x());
        assert!(circle_bounds.width() >= 22.0);
        assert!(last_circle_bounds.x() + last_circle_bounds.width() < reload_bounds.x());

        calendars[0].enabled = true;
        calendars[1].enabled = false;
        imp.calendar_toggles.update(&page, calendars.clone());
        assert_eq!(imp.calendars.first_child(), Some(first.clone().upcast()));
        assert!(first.is_active());
        assert!(!second.is_active());

        let warning = first.child().unwrap().last_child().unwrap();
        assert!(!warning.is_visible());
        calendars[0].sync_error = Some("Connection refused\nServer unavailable".into());
        imp.calendar_toggles.update(&page, calendars.clone());
        assert_eq!(imp.calendars.first_child(), Some(first.clone().upcast()));
        assert!(warning.is_visible());
        assert_eq!(
            first.tooltip_text().as_deref(),
            Some("Same name\nConnection refused\nServer unavailable")
        );
        assert_eq!(second.tooltip_text().as_deref(), Some("Same name"));
        calendars[0].sync_error = None;
        imp.calendar_toggles.update(&page, calendars.clone());
        assert!(!warning.is_visible());
        assert_eq!(first.tooltip_text().as_deref(), Some("Same name"));

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
