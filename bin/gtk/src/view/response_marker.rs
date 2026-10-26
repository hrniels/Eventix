// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, OnceCell, RefCell};

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
    let marker = Marker::new(size);
    marker.update(background, calendar, response, locale);
    marker.widget
}

/// Reuses the calendar dot and response widgets as a presentation changes.
pub(super) struct Marker {
    pub widget: gtk::Overlay,
    dot: gtk::DrawingArea,
    inner: OnceCell<(gtk::Box, gtk::Image)>,
    size: i32,
    background: RefCell<String>,
    response: Cell<Option<Response>>,
}

impl Marker {
    pub fn new(size: i32) -> Self {
        let dot = gtk::DrawingArea::new();
        dot.set_size_request(size, size);
        let marker = gtk::Overlay::new();
        marker.set_valign(gtk::Align::Center);
        marker.set_child(Some(&dot));
        Self {
            widget: marker,
            dot,
            inner: OnceCell::new(),
            size,
            background: RefCell::default(),
            response: Cell::new(None),
        }
    }

    pub fn update(
        &self,
        background: &str,
        calendar: &str,
        response: Option<Response>,
        locale: &dyn Locale,
    ) {
        if *self.background.borrow() != background {
            set_dot_color(
                &self.dot,
                gdk::RGBA::parse(background).unwrap_or(gdk::RGBA::TRANSPARENT),
            );
            self.background.replace(background.into());
            self.dot.queue_draw();
        }
        if let Some((inner, _)) = self.inner.get() {
            inner.set_visible(response.is_some());
        }
        let changed = self.response.replace(response) != response;
        let Some(response) = response else {
            self.widget.set_tooltip_text(Some(calendar));
            return;
        };
        let (inner, image) = self.inner.get_or_init(|| {
            let inner = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            inner.set_halign(gtk::Align::Center);
            inner.set_valign(gtk::Align::Center);
            inner.set_size_request(self.size - 6, self.size - 6);
            let image = gtk::Image::new();
            image.set_pixel_size(self.size - 6);
            inner.append(&image);
            self.widget.add_overlay(&inner);
            (inner, image)
        });
        let (name, class, title) = match response {
            Response::Accepted => (Some("check-lg"), "ev_details_good", "Accepted"),
            Response::Declined => (Some("x-lg"), "ev_details_critical", "Declined"),
            Response::Tentative => (Some("question-lg"), "ev_details_warn", "Tentative"),
            Response::Pending => (None, "ev_response_pending", "NEEDS-ACTION"),
        };
        if changed {
            inner.set_css_classes(&["ev_response_marker_inner", class]);
            image.set_visible(name.is_some());
            if let Some(name) = name {
                let resource = format!("{}/icons/{name}-symbolic.svg", crate::RESOURCE_PREFIX);
                image.set_resource(Some(&resource));
            }
        }
        self.widget
            .set_tooltip_text(Some(&format!("{calendar}: {}", locale.translate(title))));
    }
}
