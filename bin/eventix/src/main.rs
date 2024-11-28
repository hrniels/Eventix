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
use pages::{details, edit, monthly, new, weekly};
use std::{env, path::PathBuf, sync::Arc};
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

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            env::var("RUST_LOG").unwrap_or_default(),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    let mut store = CalStore::default();
    store.add(
        CalSource::new_from_dir(
            PathBuf::from(
                "/home/hrniels/.config/vdirsyncer-test/calendar-scso/calendar~jt7D6iFxhujcnk_A7SJmbFq",
            ),
            "scriptsolution".to_string(),
        )
        .expect("Loading calendar failed"),
    );
    store.add(
        CalSource::new_from_dir(
            PathBuf::from(
                "/home/hrniels/.config/vdirsyncer-test/tasks-scso/tasks~h26VsoBEVm_nzi6FeDvGvES",
            ),
            "scriptsolution_tasks".to_string(),
        )
        .expect("Loading calendar failed"),
    );
    store.add(
        CalSource::new_from_dir(
            PathBuf::from("/home/hrniels/.config/vdirsyncer-test/calendar-bi/calendar"),
            "bi".to_string(),
        )
        .expect("Loading calendar failed"),
    );
    store.add(
        CalSource::new_from_dir(
            PathBuf::from(
                "/home/hrniels/.config/vdirsyncer-test/calendar-holidays/iorbbNt57wxpN2K3",
            ),
            "holidays".to_string(),
        )
        .expect("Loading calendar failed"),
    );
    store.add(
        CalSource::new_from_dir(
            PathBuf::from(
                "/home/hrniels/.config/vdirsyncer-test/calendar-oschair/osstaff_shared_by_adam",
            ),
            "oschair".to_string(),
        )
        .expect("Loading calendar failed"),
    );
    store.add(
        CalSource::new_from_dir(
            PathBuf::from("/home/hrniels/.config/vdirsyncer-test/calendar-oslwl/-_shared_by_adam"),
            "oslwl".to_string(),
        )
        .expect("Loading calendar failed"),
    );

    let state = state::State::new(Arc::new(Mutex::new(store)));
    let app = Router::new()
        .nest_service("/favicon.ico", ServeFile::new("static/images/icon.png"))
        .nest_service("/static", ServeDir::new("static"))
        .nest(monthly::path(), monthly::router(state.clone()))
        .nest(weekly::path(), weekly::router(state.clone()))
        .nest(details::path(), details::router(state.clone()))
        .nest(edit::path(), edit::router(state.clone()))
        .nest(new::path(), new::router(state.clone()))
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
