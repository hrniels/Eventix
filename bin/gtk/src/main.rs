// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod event_card;
mod event_details;
mod style;
mod timeline;
mod week_page;
mod window;

use chrono::NaiveDate;
use clap::Parser;
use gtk::{gio, glib, prelude::*};

const APP_ID: &str = "io.github.hrniels.Eventix.Gtk";
const DATA_PREFIX: &str = if cfg!(debug_assertions) {
    "io.github.hrniels.Eventix-debug"
} else {
    "io.github.hrniels.Eventix"
};

/// A local, read-only GTK week view for Eventix.
#[derive(Parser, Clone)]
#[command(version, about)]
struct Args {
    /// XDG data/config prefix used by the existing Eventix installation.
    #[arg(long, default_value = DATA_PREFIX)]
    data_prefix: String,
    /// Initially displayed date (YYYY-MM-DD); defaults to today in the calendar timezone.
    #[arg(long)]
    date: Option<NaiveDate>,
}

fn main() -> glib::ExitCode {
    let args = Args::parse();
    gio::resources_register_include!("eventix.gresource")
        .expect("compiled GTK resources are valid");
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| {
        if let Some(display) = gtk::gdk::Display::default() {
            style::install(&display);
        }
    });
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
        } else {
            window::Window::new(app, args.data_prefix.clone(), args.date).present();
        }
    });
    app.run_with_args(&[] as &[&str])
}
