// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use eventix_locale::Locale;
use eventix_view::details::Response;
use gtk::{gdk, prelude::*};

use super::calendar_selector::set_dot_color;

/// Shows the invitation response inside a calendar-colored circle.
pub(super) fn new(
    background: &str,
    calendar: &str,
    response: Option<Response>,
    size: i32,
    locale: &dyn Locale,
) -> gtk::Overlay {
    let dot = gtk::DrawingArea::new();
    dot.set_size_request(size, size);
    set_dot_color(
        &dot,
        gdk::RGBA::parse(background).unwrap_or(gdk::RGBA::TRANSPARENT),
    );
    let marker = gtk::Overlay::new();
    marker.set_valign(gtk::Align::Center);
    marker.set_child(Some(&dot));
    marker.set_tooltip_text(Some(calendar));

    if let Some(response) = response {
        let (name, class, title) = match response {
            Response::Accepted => (Some("check-lg"), "ev_details_good", "Accepted"),
            Response::Declined => (Some("x-lg"), "ev_details_critical", "Declined"),
            Response::Tentative => (Some("question-lg"), "ev_details_warn", "Tentative"),
            Response::Pending => (None, "ev_response_pending", "NEEDS-ACTION"),
        };
        let inner = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        inner.set_halign(gtk::Align::Center);
        inner.set_valign(gtk::Align::Center);
        inner.set_size_request(size - 6, size - 6);
        inner.add_css_class("ev_response_marker_inner");
        inner.add_css_class(class);
        if let Some(name) = name {
            let image = gtk::Image::from_resource(&format!(
                "{}/icons/{name}-symbolic.svg",
                crate::RESOURCE_PREFIX
            ));
            image.set_pixel_size(size - 6);
            inner.append(&image);
        }
        marker.add_overlay(&inner);
        marker.set_tooltip_text(Some(&format!("{calendar}: {}", locale.translate(title))));
    }

    marker
}
