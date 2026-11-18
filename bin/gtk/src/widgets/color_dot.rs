// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{gdk, prelude::*};

/// A bordered calendar-color circle.
pub(crate) struct ColorDot {
    pub widget: gtk::DrawingArea,
}

impl ColorDot {
    pub fn new(size: i32) -> Self {
        Self {
            widget: gtk::DrawingArea::builder()
                .width_request(size)
                .height_request(size)
                .halign(gtk::Align::Center)
                .valign(gtk::Align::Center)
                .build(),
        }
    }

    pub fn from_widget(widget: gtk::DrawingArea) -> Self {
        Self { widget }
    }

    pub fn set_color(&self, color: gdk::RGBA) {
        self.widget.set_draw_func(move |_, cr, width, height| {
            draw_circle(cr, width, height, color, false);
        });
        self.widget.queue_draw();
    }

    pub fn bind_toggle(&self, button: &gtk::ToggleButton, color: gdk::RGBA) {
        let weak = button.downgrade();
        self.widget.set_draw_func(move |_, cr, width, height| {
            let Some(button) = weak.upgrade() else { return };
            let color = if button.is_active() {
                color
            } else {
                gdk::RGBA::new(0.65, 0.65, 0.65, 1.0)
            };
            draw_circle(
                cr,
                width,
                height,
                color,
                button.state_flags().contains(gtk::StateFlags::PRELIGHT),
            );
        });
        let weak = self.widget.downgrade();
        button.connect_state_flags_changed(move |_, _| {
            if let Some(dot) = weak.upgrade() {
                dot.queue_draw();
            }
        });
    }
}

fn draw_circle(cr: &gtk::cairo::Context, width: i32, height: i32, color: gdk::RGBA, hover: bool) {
    cr.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    cr.arc(
        f64::from(width) / 2.0,
        f64::from(height) / 2.0,
        f64::from(width.min(height)) / 2.0 - 1.0,
        0.0,
        std::f64::consts::TAU,
    );
    let _ = cr.fill_preserve();
    let border = if hover { 0.0 } else { 187.0 / 255.0 };
    cr.set_source_rgb(border, border, border);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}
