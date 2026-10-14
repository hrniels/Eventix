// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod actions;
mod model;
mod style;
mod view;
mod window;

use std::env;

use chrono::NaiveDate;
use clap::Parser;
use gtk::{gio, glib, prelude::*};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;

const APP_ID: &str = "io.github.hrniels.Eventix.Gtk";
const DATA_PREFIX: &str = if cfg!(debug_assertions) {
    "io.github.hrniels.Eventix-debug"
} else {
    "io.github.hrniels.Eventix"
};

/// A local GTK calendar for Eventix.
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

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            env::var("RUST_LOG").unwrap_or(String::from("info")),
        ))
        // Configure the formatting layer to include span fields
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_level(true)
                .with_span_events(FmtSpan::CLOSE)
                .compact(),
        )
        .init();

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
