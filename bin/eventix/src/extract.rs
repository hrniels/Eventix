// Copyright (C) 2025 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::body::{self, Body};
use axum::extract::FromRequest;
use axum::http::{Request, StatusCode};
use serde::de::DeserializeOwned;

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
        Ok(Self(
            // disable strict-mode to support array fields
            serde_qs::Config::new(5, false)
                .deserialize_bytes(&body)
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
        Ok(Self(
            // disable strict-mode to support array fields
            serde_qs::Config::new(5, false)
                .deserialize_str(query)
                .map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Query deserialization failed: {}", e),
                    )
                })?,
        ))
    }
}
