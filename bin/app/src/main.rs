use std::{path::Path, process::Command};

use async_channel::{Sender, unbounded};
use clap::Parser;
use gtk::{gio::ApplicationFlags, glib, prelude::*};
use ksni::blocking::TrayMethods;
use webkit2gtk::{
    NavigationPolicyDecision, NavigationPolicyDecisionExt, NavigationType, PolicyDecisionExt,
    PolicyDecisionType, SettingsExt, URIRequestExt, WebView, WebViewExt,
};

fn to_abs_path(path: &str) -> Option<String> {
    let abs = Path::new(path).canonicalize().ok()?;
    abs.to_str().map(|s| s.to_string())
}

enum TrayMessage {
    ToggleWindow,
}

struct MyTray {
    sender: Sender<TrayMessage>,
}

impl ksni::Tray for MyTray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn icon_theme_path(&self) -> String {
        to_abs_path("../eventix/static").expect("Cannot turn ../eventix/static into absolute path")
    }

    fn icon_name(&self) -> String {
        "icon".into()
    }

    fn title(&self) -> String {
        "Eventix".into()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "Exit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        if let Err(e) = self.sender.try_send(TrayMessage::ToggleWindow) {
            eprintln!("Failed to send message: {:?}", e);
        }
    }
}

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

        if !args.no_tray {
            let tray = MyTray { sender: main_tx };
            let _handle = tray.spawn().unwrap();
        }

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
            glib::MainContext::default().spawn_local(async move {
                while let Ok(msg) = main_rx.recv().await {
                    match msg {
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
    });

    // pass no arguments to GTK, because it doesn't support our application arguments above
    app.run_with_args(&[""]);
}
