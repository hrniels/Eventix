// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use anyhow::{Context, anyhow};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, StatusCode, header};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use serde::Serialize;

const MAX_RESPONSE_SIZE: usize = 1024 * 1024;

type HyperClient = Client<HttpsConnector<HttpConnector>, Full<Bytes>>;

/// A simple HTTP client based on hyper
#[derive(Clone)]
pub(crate) struct HttpClient {
    client: HyperClient,
}

/// Represents a HTTP response with status code and the received bytes
pub(crate) struct HttpResponse {
    pub(crate) status: StatusCode,
    pub(crate) body: Bytes,
}

impl HttpClient {
    /// Creates a new HTTP client
    pub(crate) fn new() -> Self {
        let connector = HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_only()
            .enable_http1()
            .build();
        let client = Client::builder(TokioExecutor::new()).build(connector);
        Self { client }
    }

    /// Performs a POST request to given URI with given timeout
    ///
    /// The `form` argument specifies the body of the post request.
    pub(crate) async fn post_form<T>(
        &self,
        uri: &str,
        form: &T,
        timeout: Duration,
    ) -> anyhow::Result<HttpResponse>
    where
        T: Serialize + ?Sized,
    {
        let body = serde_urlencoded::to_string(form).context("encoding HTTP form")?;
        let request = Request::post(uri)
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Full::new(Bytes::from(body)))
            .context("building HTTP request")?;

        tokio::time::timeout(timeout, async {
            let response = self
                .client
                .request(request)
                .await
                .context("sending HTTP request")?;
            let status = response.status();
            let body = Limited::new(response.into_body(), MAX_RESPONSE_SIZE)
                .collect()
                .await
                .map_err(|error| anyhow!(error))
                .context("reading HTTP response")?
                .to_bytes();
            Ok(HttpResponse { status, body })
        })
        .await
        .context("HTTP request timed out")?
    }
}
