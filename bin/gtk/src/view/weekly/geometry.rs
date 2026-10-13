// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{gdk, graphene, prelude::*};

pub(super) const GUTTER: i32 = 64;
pub(super) const MIN_WIDTH: i32 = 840;

/// Shared day boundaries keep headers, timed cards and strip spans aligned after rounding.
pub(super) fn day_x(width: i32, day: f64) -> f64 {
    f64::from(GUTTER) + f64::from(width - GUTTER) * day / 7.0
}

pub(super) fn rect(snapshot: &gtk::Snapshot, color: &gdk::RGBA, x: f32, y: f32, w: f32, h: f32) {
    snapshot.append_color(color, &graphene::Rect::new(x, y, w, h));
}

pub(super) fn draw_text(
    widget: &gtk::Widget,
    snapshot: &gtk::Snapshot,
    text: &str,
    x: f32,
    y: f32,
    width: f32,
) {
    let layout = widget.create_pango_layout(Some(text));
    layout.set_width((width * gtk::pango::SCALE as f32) as i32);
    layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
    snapshot.save();
    snapshot.translate(&graphene::Point::new(x, y));
    snapshot.append_layout(&layout, &widget.color());
    snapshot.restore();
}
