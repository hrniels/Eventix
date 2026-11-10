// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

include!(concat!(env!("OUT_DIR"), "/resources.rs"));

mod actions;
mod model;
mod presentation;
mod style;
mod tray;
mod view;
mod widgets;
mod window;

use std::{env, ffi::OsString};

use chrono::NaiveDate;
use clap::Parser;
use gtk::{gio, glib, prelude::*};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;

/// A local GTK calendar for Eventix.
#[derive(Parser, Clone)]
#[command(version, about)]
struct Args {
    /// XDG data/config prefix used by the existing Eventix installation.
    #[arg(long, default_value = APP_ID)]
    data_prefix: String,
    /// Initially displayed date (YYYY-MM-DD); defaults to today in the calendar timezone.
    #[arg(long)]
    date: Option<NaiveDate>,
    /// Disable the system tray icon.
    #[arg(long)]
    no_tray: bool,
    /// iCalendar files to preview and import.
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    files: Vec<OsString>,
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

    let app = application(&args, APP_ID);

    // Let GApplication resolve paths/URIs and forward file opens to the primary instance.
    // Clap-only options have already been consumed and must not reach GApplication's parser.
    let mut application_args = vec![String::from("eventix-gtk")];
    if !args.files.is_empty() {
        application_args.push("--".into());
        application_args.extend(
            args.files
                .iter()
                .map(|file| gio::File::for_commandline_arg(file).uri().to_string()),
        );
    }
    app.run_with_args(&application_args)
}

fn application(args: &Args, id: &str) -> gtk::Application {
    let app = gtk::Application::builder()
        .application_id(id)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    app.connect_startup(|_| {
        if let Some(display) = gtk::gdk::Display::default() {
            style::install(&display);
        }
    });

    let activation_args = args.clone();
    app.connect_activate(move |app| {
        main_window(app, &activation_args).present_from_tray();
    });

    let open_args = args.clone();
    app.connect_open(move |app, files, _| {
        main_window(app, &open_args).open_imports(files);
    });
    app
}

fn main_window(app: &gtk::Application, args: &Args) -> window::Window {
    if let Some(window) = app
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<window::Window>().ok())
    {
        return window;
    }

    let window = window::Window::new(app, args.data_prefix.clone(), args.date);
    if !args.no_tray {
        window.setup_tray();
    }
    window
}
