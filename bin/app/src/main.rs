// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    env, fs,
    net::TcpStream,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use async_channel::unbounded;
use clap::Parser;
use gtk::{glib, prelude::*};
use ksni::blocking::{Handle, TrayMethods};
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use webkit6::{
    NavigationPolicyDecision, PolicyDecisionType, WebContext, WebView,
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

#[derive(Serialize, Deserialize, Debug)]
struct WindowState {
    width: i32,
    height: i32,
    maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1400,
            height: 900,
            maximized: false,
        }
    }
}

fn server_is_reachable(address: &str, port: u16) -> bool {
    TcpStream::connect((address, port)).is_ok()
}

fn stop_spawned_server(child: &mut Child) {
    if let Ok(None) = child.try_wait() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn ensure_webserver_running(args: &Args) -> Option<Child> {
    // already running? use that server
    if server_is_reachable(&args.address, args.port) {
        return None;
    }

    // start the server
    let mut cmd = Command::new("eventix");
    cmd.arg("--address")
        .arg(&args.address)
        .arg("--port")
        .arg(args.port.to_string())
        .stdin(Stdio::null());
    let mut child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("spawning eventix server failed: {e:?}"));

    // give a server some time to listen on the socket
    for _ in 0..50 {
        if server_is_reachable(&args.address, args.port) {
            return Some(child);
        }

        if let Ok(Some(status)) = child.try_wait() {
            panic!("eventix server exited before startup finished: {status}");
        }

        thread::sleep(Duration::from_millis(100));
    }

    stop_spawned_server(&mut child);
    panic!(
        "eventix server did not start listening on {}:{}",
        args.address, args.port
    );
}

fn main() {
    let args = Args::parse();

    let xdg = BaseDirectories::with_prefix(APP_ID);

    // Change directory to something that is definitively visible. This seems to be a weird issue
    // of webkit leading to the following problem if, for example, being embedded in a flatpak
    // application that is started from the home directory (which is not mounted in the sandbox):
    //
    //   Connection: failed to receive credentials: Expecting to read a single byte for receiving
    //     credentials but read zero bytes
    //
    // My guess is that webkit tries to detect whether it's running in a sandbox and that detection
    // is brittle. Apparently the problem can be avoided by changing the current directory to some
    // other place that is mounted in the sandbox.
    //
    // Other people had similar issues:
    // - https://github.com/hugolabe/Wike/issues/239#issuecomment-4300133558
    if let Some(data_home) = xdg.get_data_home() {
        let _ = env::set_current_dir(data_home);
    }

    let spawned_server = Arc::new(Mutex::new(ensure_webserver_running(&args)));
    let app = gtk::Application::builder().application_id(APP_ID).build();

    app.connect_shutdown({
        let spawned_server = spawned_server.clone();
        move |_| {
            if let Some(mut child) = spawned_server.lock().unwrap().take() {
                stop_spawned_server(&mut child);
            }
        }
    });

    app.connect_activate(move |app| {
        // create channel between tray icon and main GTK thread
        let (main_tx, main_rx) = unbounded();

        let icon = xdg.find_data_file("static/icon.png").unwrap();
        let tray = if !args.no_tray {
            let tray = EventixTray::new(main_tx, icon.clone());
            match tray.disable_dbus_name(true).spawn() {
                Ok(t) => Some(Arc::new(Mutex::new(t))),
                Err(e) => {
                    println!("Spawning tray failed: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        let state_path = xdg
            .place_config_file("window_state.json")
            .expect("place state file");
        let state: WindowState = fs::read_to_string(&state_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(state.width)
            .default_height(state.height)
            .title("Eventix")
            .build();

        if state.maximized {
            window.maximize();
        }

        let context = WebContext::new();
        context.set_cache_model(webkit6::CacheModel::DocumentViewer);
        let webview = WebView::builder().web_context(&context).build();
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

        window.connect_close_request(move |window| {
            let (width, height) = window.default_size();
            let state = WindowState {
                width,
                height,
                maximized: window.is_maximized(),
            };
            if let Ok(s) = serde_json::to_string(&state) {
                let _ = fs::write(&state_path, s);
            }
            glib::Propagation::Proceed
        });

        // handle messages in main GTK thread
        if let Some(tray) = tray {
            let app = app.clone();
            let base_url = url.clone();
            let mut maximized = false;
            glib::MainContext::default().spawn_local(async move {
                while let Ok(msg) = main_rx.recv().await {
                    match msg {
                        TrayMessage::LoadPage(uri) => {
                            if !window.is_visible() {
                                if maximized {
                                    window.maximize();
                                }
                                window.present();
                            }
                            webview.load_uri(&format!("{base_url}{uri}"));
                        }
                        TrayMessage::ToggleWindow => {
                            if window.is_visible() {
                                maximized = window.is_maximized();
                                window.set_visible(false);
                            } else {
                                if maximized {
                                    window.maximize();
                                }
                                window.present();
                            }
                        }
                        TrayMessage::Quit => app.quit(),
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
