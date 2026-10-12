// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::{BTreeSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use eventix_view::WeekSnapshot;
use gtk::{gdk, prelude::*};

pub fn install(display: &gdk::Display) {
    let provider = gtk::CssProvider::new();
    provider.load_from_resource("/io/github/hrniels/Eventix/gtk/style.css");
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// Resolves a CSS-defined drawing color, falling back to the widget's foreground.
// GTK 4.10 has no replacement for looking up named CSS colors through StyleContext.
#[allow(deprecated)]
pub fn color(widget: &impl IsA<gtk::Widget>, name: &str) -> gdk::RGBA {
    widget
        .style_context()
        .lookup_color(name)
        .unwrap_or_else(|| widget.color())
}

pub fn calendar_class(background: &str, foreground: &str) -> String {
    let mut hash = DefaultHasher::new();
    (background, foreground).hash(&mut hash);
    format!("ev_color_{}", hash.finish())
}

/// Builds only the data-dependent calendar color rules; all other styling lives in style.css.
pub fn calendar_css(week: &WeekSnapshot) -> String {
    let mut css = String::new();
    let colors: BTreeSet<_> = week
        .days
        .iter()
        .flat_map(|d| d.all_day.iter().chain(&d.timed))
        .map(|e| (&e.background, &e.foreground))
        .collect();
    for (background, foreground) in colors {
        // Parse colors before generating CSS, so calendar data cannot inject styling.
        let bg = background
            .parse::<gdk::RGBA>()
            .unwrap_or(gdk::RGBA::new(0.8, 0.88, 0.98, 1.0));
        let fg = foreground.parse::<gdk::RGBA>().unwrap_or(gdk::RGBA::BLACK);
        css.push_str(&format!(
            "button.{} {{ background: {bg}; color: {fg}; }}\n",
            calendar_class(background, foreground)
        ));
    }
    css
}
