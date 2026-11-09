// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{gdk, prelude::*};

use super::ColorDot;

/// A calendar visibility toggle with an optional contrasting error mark.
pub(crate) struct CalendarToggle {
    pub widget: gtk::ToggleButton,
    warning: gtk::DrawingArea,
    background: String,
}

impl CalendarToggle {
    pub fn new(name: &str, background: &str, size: i32) -> Self {
        let builder = gtk::Builder::from_resource(resource_path!("widgets/calendar_toggle.ui"));
        let widget: gtk::ToggleButton = builder.object("calendar_toggle").expect("calendar toggle");
        let warning = builder.object("warning").expect("calendar warning");
        let dot = ColorDot::from_widget(builder.object("dot").expect("calendar dot"));
        dot.widget.set_size_request(size, size);
        widget.set_tooltip_text(Some(name));
        widget.update_property(&[gtk::accessible::Property::Label(name)]);
        dot.bind_toggle(
            &widget,
            background.parse().unwrap_or(gdk::RGBA::TRANSPARENT),
        );
        Self {
            widget,
            warning,
            background: background.into(),
        }
    }

    pub fn update(&self, name: &str, active: bool, error: Option<&str>) {
        self.widget.set_active(active);
        self.widget
            .update_property(&[gtk::accessible::Property::Label(name)]);
        let tooltip = error.map_or_else(|| name.to_owned(), |error| format!("{name}\n{error}"));
        self.widget.set_tooltip_text(Some(&tooltip));
        self.warning.set_visible(error.is_some());
        let color = sync_error_color(&self.background, active);
        self.warning.set_draw_func(move |_, cr, width, height| {
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
        self.warning.queue_draw();
    }
}

/// Chooses a contrasting foreground, preferring white on darker mid-tone circles.
fn sync_error_color(background: &str, enabled: bool) -> gdk::RGBA {
    let background = if enabled {
        background.parse().unwrap_or(gdk::RGBA::BLACK)
    } else {
        gdk::RGBA::new(0.65, 0.65, 0.65, 1.0)
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
        gdk::RGBA::WHITE
    } else {
        gdk::RGBA::BLACK
    }
}

#[cfg(test)]
mod tests {
    use super::sync_error_color;
    use gtk::gdk;

    #[test]
    fn error_mark_contrasts_with_calendar_colors_and_inactive_gray() {
        for background in ["#000000", "#0000ff", "#333333", "#ff0000", "#ed3346"] {
            assert_eq!(sync_error_color(background, true), gdk::RGBA::WHITE);
        }
        for background in ["#ffffff", "#ffff00", "#bbbbbb"] {
            assert_eq!(sync_error_color(background, true), gdk::RGBA::BLACK);
        }
        for background in ["#000000", "#ffffff", "#0000ff"] {
            assert_eq!(sync_error_color(background, false), gdk::RGBA::BLACK);
        }
    }
}
