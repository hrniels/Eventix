// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod attendees;
pub mod auth;
pub mod calendars;
pub mod collections;
pub mod help;
pub mod items;
pub mod setlang;
pub mod togglecal;

use anyhow::Chain;
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{HeaderValue, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use eventix_state::EventixState;
use serde_json::json;

#[derive(Debug)]
pub struct JsonError {
    inner: anyhow::Error,
}

impl JsonError {
    fn generate_message(&self) -> String {
        let mut msg = format!("{}", self.inner);

        if let Some(cause) = self.inner.source() {
            msg.push_str("\n\nCaused by:");
            for (n, error) in Chain::new(cause).enumerate() {
                msg.push_str(&format!("\n  {}: {}", n + 1, error));
            }
        }

        msg
    }
}

impl From<anyhow::Error> for JsonError {
    fn from(err: anyhow::Error) -> Self {
        Self { inner: err }
    }
}

impl IntoResponse for JsonError {
    fn into_response(self) -> Response {
        tracing::debug!("request failed: {:?}", self.inner);

        let body = Json(json!({
            "error": self.generate_message(),
        }));

        // use a temporary and otherwise unused error code to simply keep the body below
        (StatusCode::CONTINUE, body).into_response()
    }
}

async fn json_error_middleware(req: Request<Body>, next: Next) -> Response {
    let res = next.run(req).await;

    if !res.status().is_success() {
        // extract the body bytes (consumes it)
        let status = res.status();

        let mut resp = match status {
            StatusCode::CONTINUE => {
                (StatusCode::INTERNAL_SERVER_ERROR, res.into_body()).into_response()
            }
            status => {
                let bytes = to_bytes(res.into_body(), 1024 * 16).await;

                // build a new JSON body
                let msg = match bytes {
                    Ok(b) => match String::from_utf8(b.to_vec()) {
                        Ok(s) if !s.is_empty() => s,
                        _ => status
                            .canonical_reason()
                            .unwrap_or("Unknown error")
                            .to_string(),
                    },
                    Err(_) => status
                        .canonical_reason()
                        .unwrap_or("Unknown error")
                        .to_string(),
                };

                let json_body = Json(json!({ "error": msg }));
                (status, json_body).into_response()
            }
        };

        resp.headers_mut().append(
            "Content-Type",
            HeaderValue::from_str("application/json").unwrap(),
        );

        return resp;
    } else {
        tracing::debug!("got status {}", res.status());
    }

    res
}

async fn error_handler() -> impl IntoResponse {
    JsonError::from(anyhow::Error::msg("no such route"))
}

pub fn router(state: EventixState) -> Router {
    Router::new()
        .with_state(state.clone())
        .nest("/calendars", calendars::router(state.clone()))
        .nest("/collections", collections::router(state.clone()))
        .nest("/items", items::router(state.clone()))
        .merge(attendees::router(state.clone()))
        .merge(auth::router(state.clone()))
        .merge(help::router(state.clone()))
        .merge(togglecal::router(state.clone()))
        .merge(setlang::router(state.clone()))
        .fallback(error_handler)
        .layer(axum::middleware::from_fn(json_error_middleware))
}
