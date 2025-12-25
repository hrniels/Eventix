mod api;
mod comps;
mod debug;
mod extract;
mod generated;
mod html;
mod notify;
mod objects;
mod pages;
mod util;

use axum::{
    Router,
    body::Body,
    http::{HeaderValue, Request, header},
    response::IntoResponse,
};
use clap::Parser;
use pages::error::HTMLError;
use std::{env, panic, sync::Arc};
use tokio::{net::TcpListener, sync::Mutex};
use tower_http::{
    LatencyUnit,
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::{fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt};
use xdg::BaseDirectories;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

async fn error_handler() -> impl IntoResponse {
    HTMLError::from(anyhow::Error::msg("no such route"))
}

/// A website to manage iCalendar events and tasks
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// the address for the webserver
    #[arg(long, default_value = "127.0.0.1")]
    address: String,

    /// the port number for the webserver
    #[arg(long, default_value_t = 8084)]
    port: u16,
}

async fn run_server(listener: TcpListener) {
    let xdg = Arc::new(BaseDirectories::with_prefix(APP_ID));

    let state = eventix_state::State::new(xdg.clone()).expect("loading state");
    let locale = state.settings().locale();
    let state = Arc::new(Mutex::new(state));

    let icon_path = xdg
        .find_data_file("static/icon.png")
        .expect("Find '$XDG_DATA_HOME/static/icon.png'");
    let static_path = icon_path.parent().unwrap().to_owned();

    let app = Router::new()
        .route_service("/favicon.ico", ServeFile::new(icon_path))
        .nest_service("/static", ServeDir::new(static_path.clone()))
        .merge(pages::monthly::router(state.clone()))
        .nest("/api", api::router(state.clone()))
        .nest("/generated", generated::router(state.clone(), static_path))
        .nest("/pages", pages::router(state.clone()))
        .fallback(error_handler)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
        .layer(debug::TraceReqDetailsLayer)
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
                .on_request(|_req: &Request<Body>, _span: &tracing::Span| {
                    tracing::debug!("enter");
                })
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::DEBUG)
                        .latency_unit(LatencyUnit::Micros),
                ),
        );

    // start helper tasks
    tokio::spawn(notify::watch_alarms(state.clone(), locale));
    let nstate = state.clone();
    tokio::spawn(async move {
        eventix_cmd::handle_commands(&xdg, nstate)
            .await
            .expect("cmds failed")
    });

    axum::serve(listener, app).await.unwrap();
}

#[tokio::main]
async fn main() {
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

    let args = Args::parse();

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.address, args.port)).await;
    match listener {
        Ok(listener) => run_server(listener).await,

        Err(e) => {
            panic!("bind to {}:{} failed: {:?}", args.address, args.port, e);
        }
    }
}
