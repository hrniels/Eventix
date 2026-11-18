// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

include!(concat!(env!("OUT_DIR"), "/resources.rs"));

mod actions;
mod autostart;
mod dates;
mod model;
mod style;
mod tray;
mod view;
mod widgets;
mod window;
mod worker;

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
    /// Start with only the tray icon visible.
    #[arg(
        long,
        conflicts_with = "no_tray",
        help = eventix_locale::default().translate("Start with only the tray icon visible").to_owned()
    )]
    background: bool,
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
    // Only forward options understood by GApplication; --background must reach the primary instance.
    let mut application_args = vec![String::from("eventix-gtk")];
    if args.background {
        application_args.push("--background".into());
    }
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
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.add_main_option(
        "background",
        glib::Char::from(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        eventix_locale::default().translate("Start with only the tray icon visible"),
        None,
    );
    app.connect_startup(|_| {
        if let Some(display) = gtk::gdk::Display::default() {
            style::install(&display);
        }
    });

    let activation_args = args.clone();
    app.connect_activate(move |app| {
        main_window(app, &activation_args).present_from_tray();
    });

    let command_args = args.clone();
    app.connect_command_line(move |app, command| {
        let background = command
            .options_dict()
            .lookup::<bool>("background")
            .ok()
            .flatten()
            .unwrap_or(false);
        let files: Vec<_> = command
            .arguments()
            .iter()
            .skip(1)
            .filter(|arg| *arg != "--background" && *arg != "--")
            .map(|arg| command.create_file_for_arg(arg))
            .collect();
        let window = main_window(app, &command_args);
        if !files.is_empty() {
            window.open_imports(&files);
        } else if !background {
            window.present_from_tray();
        }
        glib::ExitCode::SUCCESS
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
    if !args.no_tray && !window.setup_tray() {
        // Never leave the user with an inaccessible application if tray startup fails.
        window.present_from_tray();
    }
    window
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use gtk::{gio, glib, prelude::*};

    use super::Args;

    #[test]
    fn background_start_requires_the_tray_and_defaults_to_disabled() {
        assert!(!Args::try_parse_from(["eventix-gtk"]).unwrap().background);
        assert!(
            Args::try_parse_from(["eventix-gtk", "--background"])
                .unwrap()
                .background
        );
        assert!(Args::try_parse_from(["eventix-gtk", "--background", "--no-tray"]).is_err());
    }

    #[test]
    #[ignore = "requires a GTK display"]
    fn background_start_stays_hidden_until_normal_activation() {
        gtk::init().unwrap();
        gio::resources_register_include!("eventix.gresource").unwrap();
        let args = Args::try_parse_from(["eventix-gtk", "--background"]).unwrap();
        let app = super::application(&args, "io.github.hrniels.Eventix.StartupTest");
        app.connect_startup(|app| {
            let (window, _) = crate::window::Window::test_new(app);
            assert!(!window.is_visible());
        });
        let weak = app.downgrade();
        glib::idle_add_local_once(move || {
            let app = weak.upgrade().unwrap();
            let windows = app.windows();
            assert_eq!(windows.len(), 1);
            assert!(!windows[0].is_visible());
            app.activate();
            assert!(windows[0].is_visible());
            assert_eq!(app.windows().len(), 1);
            app.quit();
        });
        assert_eq!(
            app.run_with_args(&["eventix-gtk", "--background"]),
            glib::ExitCode::SUCCESS
        );
    }
}
