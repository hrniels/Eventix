// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use async_channel::unbounded;
use clap::Parser;
use gtk::{glib, prelude::*};
use ksni::blocking::{Handle, TrayMethods};
use tokio::runtime::Runtime;
use webkit6::{
    NavigationPolicyDecision, PolicyDecisionType, WebView,
    prelude::{PolicyDecisionExt, WebViewExt},
};
use xdg::BaseDirectories;

use crate::tray::{EventixTray, TaskStatus, TrayMessage};

mod tray;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// GTK frontend for the eventix server
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// the address for the eventix server
    #[arg(long, default_value = "127.0.0.1")]
    address: String,

    /// the port number for the eventix server
    #[arg(long, default_value_t = 8084)]
    port: u16,

    /// disable system tray icon
    #[arg(long)]
    no_tray: bool,
}

fn main() {
    let args = Args::parse();

    let xdg = BaseDirectories::with_prefix(APP_ID);
    let app = gtk::Application::builder().application_id(APP_ID).build();

    app.connect_activate(move |app| {
        // create channel between tray icon and main GTK thread
        let (main_tx, main_rx) = unbounded();

        let icon = xdg.find_data_file("static/icon.png").unwrap();
        let tray = if !args.no_tray {
            let tray = EventixTray::new(main_tx, icon.clone());
            match tray.spawn() {
                Ok(t) => Some(Arc::new(Mutex::new(t))),
                Err(e) => {
                    println!("Spawning tray failed: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(1400)
            .default_height(900)
            .title("Eventix")
            .build();

        let webview = WebView::new();
        let settings = WebViewExt::settings(&webview).expect("webview settings");
        settings.set_enable_developer_extras(true);
        // smooth scrolling feels really laggy, so disable it
        settings.set_enable_smooth_scrolling(false);
        settings.set_enable_write_console_messages_to_stdout(true);

        let url = format!("http://{}:{}", args.address, args.port);
        let base_url = url.clone();

        // overwrite policy for clicked links
        webview.connect_decide_policy(move |_webview, decision, decision_type| {
            if decision_type == PolicyDecisionType::NavigationAction
                && let Some(nav_decision) = decision.downcast_ref::<NavigationPolicyDecision>()
                && let Some(action) = nav_decision.navigation_action()
                && let Some(request) = action.request()
                && let Some(uri) = request.uri()
                && !uri.starts_with(&base_url)
                && action.navigation_type() == webkit6::NavigationType::LinkClicked
            {
                let _ = Command::new("xdg-open").arg(uri.as_str()).spawn();
                // tell WebKit not to handle it internally
                decision.ignore();
                return true;
            }
            // let WebKit handle it by default
            false
        });

        webview.load_uri(&url);
        window.set_child(Some(&webview));

        window.present();

        // handle messages in main GTK thread
        if let Some(tray) = tray {
            let base_url = url.clone();
            glib::MainContext::default().spawn_local(async move {
                while let Ok(msg) = main_rx.recv().await {
                    match msg {
                        TrayMessage::LoadPage(uri) => {
                            if !window.is_visible() {
                                window.present();
                            }
                            webview.load_uri(&format!("{base_url}{uri}"));
                        }
                        TrayMessage::ToggleWindow => {
                            if window.is_visible() {
                                window.set_visible(false);
                            } else {
                                window.present();
                            }
                        }
                    }
                }
            });

            // Background thread to simulate task state changes
            thread::spawn({
                let tray = tray.clone();
                let xdg = xdg.clone();
                move || {
                    let mut last = None;
                    loop {
                        last = update_icon(&xdg, &tray, last.as_ref());

                        thread::sleep(Duration::from_secs(30));
                    }
                }
            });
        }
    });

    // pass no arguments to GTK, because it doesn't support our application arguments above
    app.run_with_args(&[] as &[&str]);
}

fn update_icon(
    xdg: &BaseDirectories,
    tray: &Arc<Mutex<Handle<EventixTray>>>,
    last: Option<&eventix_cmd::Response>,
) -> Option<eventix_cmd::Response> {
    let rt = Runtime::new().unwrap();
    let Ok(resp) =
        rt.block_on(async { eventix_cmd::send(xdg, eventix_cmd::Request::TaskStatus).await })
    else {
        return None;
    };

    let eventix_cmd::Response::TaskStatus(today, overdue) = resp else {
        return None;
    };
    if last.is_some() && last.unwrap() == &resp {
        return Some(resp);
    }

    let tray_lock = tray.lock().unwrap();
    tray_lock.update(|t| {
        if overdue > 0 {
            t.set_status(TaskStatus::Overdue(overdue));
        } else if today > 0 {
            t.set_status(TaskStatus::DueToday(today));
        } else {
            t.set_status(TaskStatus::None);
        }
    });
    Some(resp)
}
