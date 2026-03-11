// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{
    body::{Body, Bytes},
    http::Request,
    response::Response,
};
use futures::future::BoxFuture;
use std::task::{Context, Poll};
use tower::{Layer, Service};
use tracing::Level;

#[derive(Clone)]
pub struct TraceReqDetailsLayer;

impl<S> Layer<S> for TraceReqDetailsLayer {
    type Service = TraceReqDetailsMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TraceReqDetailsMiddleware { inner }
    }
}

#[derive(Clone)]
pub struct TraceReqDetailsMiddleware<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for TraceReqDetailsMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let (parts, body) = req.into_parts();

            // Capture and buffer the body
            let bytes: Bytes = axum::body::to_bytes(body, usize::MAX)
                .await
                .unwrap_or_default();

            // pretty-print headers and body
            if tracing::enabled!(Level::TRACE) {
                let content_type = parts
                    .headers
                    .get(axum::http::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                let pretty_body = if content_type.contains("application/x-www-form-urlencoded") {
                    serde_urlencoded::from_bytes::<Vec<(String, String)>>(&bytes)
                        .map(|form| format!("{:#?}", form))
                        .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).to_string())
                } else {
                    String::from_utf8_lossy(&bytes).to_string()
                };

                tracing::trace!(
                    "starting: headers = {:#?},\nbody = {}",
                    parts.headers,
                    pretty_body,
                );
            }

            // call handler for this request
            let req = Request::from_parts(parts, Body::from(bytes));
            inner.call(req).await
        })
    }
}
