// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{gdk, graphene, prelude::*};

pub(super) const GUTTER: i32 = 64;
pub(super) const MIN_WIDTH: i32 = 840;

/// Shared day boundaries keep calendar headers and cards aligned after rounding.
pub(super) fn day_x(width: i32, day: f64) -> f64 {
    f64::from(GUTTER) + f64::from(width - GUTTER) * day / 7.0
}

pub(super) fn rect(snapshot: &gtk::Snapshot, color: &gdk::RGBA, x: f32, y: f32, w: f32, h: f32) {
    snapshot.append_color(color, &graphene::Rect::new(x, y, w, h));
}
