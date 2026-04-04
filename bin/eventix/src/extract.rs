// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::body::{self, Body};
use axum::extract::FromRequest;
use axum::http::{Request, StatusCode};
use serde::de::DeserializeOwned;

/// Converts an `application/x-www-form-urlencoded` body/query string to the plain query-string
/// format that serde_qs expects in its default mode.
///
/// Replaces `+` with `%20` and expands `%5B`/`%5D` (case-insensitive) to literal
/// `[`/`]`, which are the bracket characters serde_qs uses to denote nesting.
fn normalize_encoding(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b'+' => {
                out.extend_from_slice(b"%20");
                i += 1;
            }
            b'%' if i + 2 < input.len() => {
                let hi = input[i + 1].to_ascii_uppercase();
                let lo = input[i + 2].to_ascii_uppercase();
                if hi == b'5' && lo == b'B' {
                    out.push(b'[');
                    i += 3;
                } else if hi == b'5' && lo == b'D' {
                    out.push(b']');
                    i += 3;
                } else {
                    out.push(input[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

#[derive(Debug)]
pub struct MultiForm<T>(pub T);

impl<T, S> FromRequest<S> for MultiForm<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = (StatusCode, String);

    async fn from_request(req: Request<Body>, _state: &S) -> Result<Self, Self::Rejection> {
        let body = body::to_bytes(req.into_body(), 32 * 1024)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        // POST bodies use application/x-www-form-urlencoded, which percent-encodes
        // brackets as %5B/%5D and encodes spaces as '+'. Normalize these to the
        // literal characters that serde_qs expects in its default (non-form) mode.
        let normalized = normalize_encoding(&body);
        Ok(Self(
            serde_qs::Config::new()
                .max_depth(5)
                .deserialize_bytes(&normalized)
                .map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!(
                            "Unable to deserialize: {}\n{}",
                            e,
                            String::from_utf8(body.to_vec()).unwrap()
                        ),
                    )
                })?,
        ))
    }
}

#[derive(Debug)]
pub struct MultiQuery<T>(pub T);

impl<T, S> FromRequest<S> for MultiQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = (StatusCode, String);

    async fn from_request(mut req: Request<Body>, _state: &S) -> Result<Self, Self::Rejection> {
        let query = req.uri_mut().query().unwrap_or("");
        // same as above: we need to decode '[' and ']'
        let normalized = normalize_encoding(query.as_bytes());
        Ok(Self(
            // Use literal brackets (not percent-encoded) for URI query strings.
            serde_qs::Config::new()
                .max_depth(5)
                .deserialize_bytes(&normalized)
                .map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Query deserialization failed: {}", e),
                    )
                })?,
        ))
    }
}
