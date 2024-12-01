mod comp;
mod comps;
mod error;
mod extract;
mod html;
mod locale;
mod notify;
mod objects;
mod pages;
mod state;

use axum::{http::Request, response::IntoResponse, Router};
use clap::Parser;
use error::HTMLError;
use ical::col::{CalSource, CalStore};
use pages::{details, edit, monthly, new, togglecal, weekly};
use serde::Deserialize;
use std::{collections::HashMap, env, fs::File, io::Read, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::{DefaultOnResponse, TraceLayer},
    LatencyUnit,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

async fn error_handler() -> impl IntoResponse {
    HTMLError::from(anyhow::Error::msg("no such route"))
}

/// A website to aggregate and analyse finance transactions
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// the address for the webserver
    #[arg(long, default_value = "0.0.0.0")]
    address: String,

    /// the port number for the webserver
    #[arg(long, default_value_t = 8081)]
    port: u16,
}

#[derive(Debug, Deserialize)]
struct Sources {
    #[serde(rename = "calendar")]
    calendars: HashMap<String, Calendar>,
}

#[derive(Debug, Deserialize)]
struct Calendar {
    path: String,
    name: String,
    disabled: Option<bool>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            env::var("RUST_LOG").unwrap_or_default(),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    let mut file = File::options()
        .read(true)
        .open("calendars.toml")
        .expect("open calendars.toml");
    let mut sources = String::new();
    file.read_to_string(&mut sources)
        .expect("read calendars.toml");
    let sources: Sources = toml::from_str(&sources).expect("parse calendars.toml");

    let mut disabled_cals = Vec::new();
    let mut store = CalStore::default();
    for (id, cal) in &sources.calendars {
        if cal.disabled.unwrap_or(false) {
            disabled_cals.push(id.clone());
        }

        store.add(
            CalSource::new_from_dir(
                Arc::from(id.clone()),
                PathBuf::from(cal.path.clone()),
                cal.name.clone(),
            )
            .expect(&format!(
                "Loading calendar {} from '{}' failed",
                id, cal.path
            )),
        );
    }

    let state = state::State::new(
        Arc::new(Mutex::new(store)),
        Arc::new(Mutex::new(disabled_cals)),
    );

    let app = Router::new()
        .nest_service("/favicon.ico", ServeFile::new("static/images/icon.png"))
        .nest_service("/static", ServeDir::new("static"))
        .nest(monthly::path(), monthly::router(state.clone()))
        .nest(weekly::path(), weekly::router(state.clone()))
        .nest(details::path(), details::router(state.clone()))
        .nest(edit::path(), edit::router(state.clone()))
        .nest(new::path(), new::router(state.clone()))
        .nest(togglecal::path(), togglecal::router(state.clone()))
        .fallback(error_handler)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    static NEXT_REQ: std::sync::Mutex<u64> = std::sync::Mutex::new(1);

                    // display a unique id to every request
                    let id = *NEXT_REQ.lock().unwrap();
                    *NEXT_REQ.lock().unwrap() = id + 1;

                    tracing::debug_span!(
                        "request",
                        method = %request.method(),
                        uri = %request.uri(),
                        id = id,
                    )
                })
                .on_response(DefaultOnResponse::new().latency_unit(LatencyUnit::Micros)),
        );

    tokio::spawn(notify::watch_alarms(
        state.clone(),
        locale::default().timezone().clone(),
    ));

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.address, args.port))
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
