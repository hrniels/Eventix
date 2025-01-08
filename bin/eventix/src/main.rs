mod comp;
mod comps;
mod error;
mod extract;
mod html;
mod locale;
mod notify;
mod objects;
mod pages;
mod settings;
mod state;

use axum::{http::Request, response::IntoResponse, Router};
use chrono::Duration;
use clap::Parser;
use error::HTMLError;
use ical::col::{CalSource, CalStore};
use pages::{attendees, complete, delete, details, edit, list, monthly, new, togglecal, weekly};
use std::{collections::HashMap, env, path::PathBuf, sync::Arc};
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

/// A website to manage iCalendar events and tasks
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

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            env::var("RUST_LOG").unwrap_or_default(),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    let settings = settings::Settings::load_from_file().expect("load settings");

    let mut disabled_cals = Vec::new();
    let mut store = CalStore::default();
    for (id, cal) in &settings.calendars {
        if cal.disabled.unwrap_or(false) {
            disabled_cals.push(id.clone());
        }

        let mut props = HashMap::new();
        props.insert("fgcolor".to_string(), cal.fgcolor.clone());
        props.insert("bgcolor".to_string(), cal.bgcolor.clone());
        if let Some(types) = &cal.types {
            props.insert("types".to_string(), serde_json::to_string(types).unwrap());
        }

        store.add(
            CalSource::new_from_dir(
                Arc::from(id.clone()),
                PathBuf::from(cal.path.clone()),
                cal.name.clone(),
                props,
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
        Arc::new(Mutex::new(
            settings
                .last_alarm_check
                .unwrap_or(chrono::Utc::now().naive_utc() - Duration::days(7)),
        )),
        Arc::new(Mutex::new(settings.last_calendar)),
    );

    let app = Router::new()
        .nest_service("/favicon.ico", ServeFile::new("static/images/icon.png"))
        .nest_service("/static", ServeDir::new("static"))
        .merge(monthly::router(state.clone()))
        .merge(weekly::router(state.clone()))
        .merge(details::router(state.clone()))
        .merge(edit::router(state.clone()))
        .merge(new::router(state.clone()))
        .merge(list::router(state.clone()))
        .merge(complete::router(state.clone()))
        .merge(delete::router(state.clone()))
        .merge(togglecal::router(state.clone()))
        .merge(attendees::router(state.clone()))
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

    tokio::spawn(notify::watch_alarms(state.clone(), locale::default()));

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.address, args.port))
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
