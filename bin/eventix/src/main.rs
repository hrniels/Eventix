mod ajax;
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
mod util;

use ajax::{attendees, complete, delete, details, occlist, reload, togglecal, toggleexcl};
use axum::{http::Request, response::IntoResponse, Router};
use clap::Parser;
use error::HTMLError;
use pages::{edit, list, monthly, new, weekly};
use std::env;
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

    let state = state::State::default();
    state.reload().await.expect("loading state");

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
        .merge(toggleexcl::router(state.clone()))
        .merge(togglecal::router(state.clone()))
        .merge(occlist::router(state.clone()))
        .merge(attendees::router(state.clone()))
        .merge(reload::router(state.clone()))
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
