use std::{
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use async_channel::unbounded;
use clap::Parser;
use gtk::{gio::ApplicationFlags, glib, prelude::*};
use ksni::blocking::{Handle, TrayMethods};
use tokio::runtime::Runtime;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, NavigationType, PolicyDecisionExt,
    PolicyDecisionType, SettingsExt, URIRequestExt, WebView, WebViewExt,
};
use xdg::BaseDirectories;

use crate::tray::{EventixTray, TaskStatus, TrayMessage};

mod tray;

/// GTK frontend for the eventix server
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// the address for the eventix server
    #[arg(long, default_value = "127.0.0.1")]
    address: String,

    /// the port number for the eventix server
    #[arg(long, default_value_t = 8081)]
    port: u16,

    /// disable system tray icon
    #[arg(long)]
    no_tray: bool,
}

fn main() {
    let args = Args::parse();

    let xdg = BaseDirectories::with_prefix("eventix");

    // try to generate a unique id so that we can have multiple instances for different eventix
    // servers (or websites in general).
    let id = format!(
        "app.eventix.a{}.p{}",
        args.address.clone().replace('.', "-"),
        args.port
    );
    let app = gtk::Application::new(Some(&id), ApplicationFlags::empty());

    app.connect_activate(move |app| {
        // create channel between tray icon and main GTK thread
        let (main_tx, main_rx) = unbounded();

        let icon = xdg.find_data_file("static/icon.png").unwrap();
        let tray = EventixTray::new(main_tx, xdg.get_data_home().unwrap(), icon);
        let tray = Arc::new(Mutex::new(tray.spawn().unwrap()));

        let window = gtk::ApplicationWindow::new(app);
        window.set_default_size(1400, 900);
        window.set_title("Eventix");
        window.set_icon_name(Some("icon"));

        let webview = WebView::new();
        if let Some(settings) = WebViewExt::settings(&webview) {
            settings.set_enable_developer_extras(true);
            // smooth scrolling feels really laggy, so disable it
            settings.set_enable_smooth_scrolling(false);
        }

        let url = format!("http://{}:{}", args.address, args.port);
        let base_url = url.clone();

        // overwrite policy for clicked links
        webview.connect_decide_policy(move |_webview, decision, decision_type| {
            if decision_type == PolicyDecisionType::NavigationAction {
                if let Some(nav_decision) = decision.dynamic_cast_ref::<NavigationPolicyDecision>()
                {
                    // TODO method is deprecated, but it's unclear to me what the replacement is
                    #[allow(deprecated)]
                    if let Some(request) = nav_decision.request() {
                        // open external URLs with xdg-open
                        if let Some(uri) = request.uri() {
                            if !uri.starts_with(&base_url) {
                                let action = nav_decision.navigation_action().unwrap();
                                if action.navigation_type() == NavigationType::LinkClicked {
                                    let _ = Command::new("xdg-open").arg(uri).spawn();
                                    // tell WebKit not to handle it internally
                                    decision.ignore();
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
            // let WebKit handle it by default
            false
        });

        webview.load_uri(&url);
        window.add(&webview);

        window.show_all();

        // handle messages in main GTK thread
        if !args.no_tray {
            let base_url = url.clone();
            glib::MainContext::default().spawn_local(async move {
                while let Ok(msg) = main_rx.recv().await {
                    match msg {
                        TrayMessage::LoadPage(uri) => {
                            if !window.is_visible() {
                                window.show_all();
                            }
                            webview.load_uri(&format!("{base_url}{uri}"));
                        }
                        TrayMessage::ToggleWindow => {
                            if window.is_visible() {
                                window.hide();
                            } else {
                                window.show_all();
                            }
                        }
                    }
                }
            });
        }

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
    });

    // pass no arguments to GTK, because it doesn't support our application arguments above
    app.run_with_args(&[""]);
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
