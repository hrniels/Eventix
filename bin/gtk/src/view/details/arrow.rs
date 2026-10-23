// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{gdk, graphene, prelude::*};

use super::EventDetails;
use crate::style;

pub(super) fn snapshot(popover: &EventDetails, contents: &gtk::Widget, snapshot: &gtk::Snapshot) {
    let Some(body) = contents.compute_bounds(popover) else {
        return;
    };
    let Some(surface) = popover.surface().and_downcast::<gdk::Popup>() else {
        return;
    };
    let top = match surface.surface_anchor() {
        gdk::Gravity::North | gdk::Gravity::NorthEast | gdk::Gravity::NorthWest => true,
        gdk::Gravity::South | gdk::Gravity::SouthEast | gdk::Gravity::SouthWest => false,
        _ => return,
    };
    let (has_anchor, anchor) = popover.pointing_to();
    let Some(target) = popover.parent().and_then(|parent| {
        parent.compute_point(
            popover,
            &graphene::Point::new(
                anchor.x() as f32 + anchor.width() as f32 / 2.0,
                anchor.y() as f32,
            ),
        )
    }) else {
        return;
    };
    if !has_anchor {
        return;
    }

    // Leave room for the body's rounded corners, including when the compositor slides it.
    let x = target
        .x()
        .clamp(body.x() + 24.0, body.x() + body.width() - 24.0);
    let y = if top {
        body.y()
    } else {
        body.y() + body.height()
    };
    triangle(
        snapshot,
        x,
        y,
        top,
        &style::color(contents, "ev_details_background"),
        &style::color(contents, "ev_details_border"),
    );
}

/// Draws a vector arrow to be covered by the body at its open base.
fn triangle(
    snapshot: &gtk::Snapshot,
    x: f32,
    y: f32,
    top: bool,
    background: &gdk::RGBA,
    border: &gdk::RGBA,
) {
    // GtkPopover's native arrow is a Cairo texture. Keep this shape as GSK primitives
    // instead, so fractional surface scaling rasterizes the arrow and body together.
    snapshot.save();
    snapshot.translate(&graphene::Point::new(x, y));
    // Adjacent antialiased edges leave a translucent seam at fractional pixels, even
    // with identical coordinates. Bleed under the opaque body, which is drawn last:
    // its border hides the bleed and defines the visible join without any overlap.
    let clip_y = if top { -12.0 } else { -1.0 };
    snapshot.push_clip(&graphene::Rect::new(-12.0, clip_y, 24.0, 13.0));
    snapshot.rotate(45.0);
    for (half_diagonal, color) in [(12.0, border), (12.0 - f32::sqrt(2.0), background)] {
        let half_side = half_diagonal / f32::sqrt(2.0);
        snapshot.append_color(
            color,
            &graphene::Rect::new(-half_side, -half_side, 2.0 * half_side, 2.0 * half_side),
        );
    }
    snapshot.pop();
    snapshot.restore();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a GTK display"]
    fn arrow_joins_body_at_fractional_scales() {
        gtk::init().unwrap();
        let renderer = gtk::gsk::CairoRenderer::new();
        renderer.realize(None::<&gdk::Surface>).unwrap();
        let mut seams = Vec::new();
        let mut overlaps = Vec::new();
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            for offset in [0.0, 0.25, 0.5, 0.75] {
                for top in [true, false] {
                    let snapshot = gtk::Snapshot::new();
                    snapshot.scale(scale, scale);
                    let y = 32.0 + offset;
                    let body_y = if top { y } else { y - 20.0 };
                    triangle(
                        &snapshot,
                        40.0 + offset,
                        y,
                        top,
                        &gdk::RGBA::WHITE,
                        &gdk::RGBA::BLACK,
                    );
                    snapshot.append_color(
                        &gdk::RGBA::new(1.0, 0.0, 0.0, 1.0),
                        &graphene::Rect::new(8.0, body_y, 64.0, 20.0),
                    );
                    let texture = renderer.render_texture(
                        snapshot.to_node().unwrap(),
                        Some(&graphene::Rect::new(0.0, 0.0, 160.0, 128.0)),
                    );
                    let mut pixels = vec![0; 160 * 128 * 4];
                    texture.download(&mut pixels, 160 * 4);
                    let center = ((40.0 + offset) * scale) as usize;
                    let edge = (y * scale) as usize;
                    for row in edge - 2..=edge + 2 {
                        for column in center - 2..=center + 2 {
                            let alpha = pixels[(row * 160 + column) * 4 + 3];
                            if alpha != 255 {
                                seams.push((scale, offset, top, row, alpha));
                            }
                        }
                    }
                    let inside = if top { edge + 2 } else { edge - 2 };
                    let pixel = (inside * 160 + center) * 4;
                    if pixels[pixel..pixel + 4] != [0, 0, 255, 255] {
                        overlaps.push((scale, offset, top));
                    }
                }
            }
        }
        renderer.unrealize();
        assert!(seams.is_empty(), "seams: {seams:?}");
        assert!(
            overlaps.is_empty(),
            "arrow painted inside body: {overlaps:?}"
        );
    }
}
