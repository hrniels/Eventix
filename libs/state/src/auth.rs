// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use anyhow::{Context, anyhow};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, header};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde::Deserialize;

use crate::{EventixState, State, SyncerType};

const CLIENT_ID: &str = "d3590ed6-52b3-4102-aeff-aad2292ab01c";
const DEVICE_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

/// A fresh device authorization, including the credential required for token polling.
#[derive(Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

/// Information shown to the user while device authentication is pending.
pub struct AuthPrompt {
    pub verification_uri: String,
    pub user_code: String,
}

/// The terminal result of a monitored authentication attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthOutcome {
    Complete,
    Declined,
    Expired,
}

/// The response to one device authorization token request.
pub enum TokenPoll {
    Pending,
    Retry,
    SlowDown,
    Complete(String),
    Declined,
    Expired,
}

type HttpClient = Client<
    hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
    Full<Bytes>,
>;

/// A reusable HTTPS client for Microsoft device authorization.
pub struct MicrosoftDeviceCodeClient {
    client: HttpClient,
}

impl Default for MicrosoftDeviceCodeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MicrosoftDeviceCodeClient {
    /// Creates a client that reuses connections across device-code and token requests.
    pub fn new() -> Self {
        let connector = HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_only()
            .enable_http1()
            .build();
        Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
        }
    }

    async fn post(&self, url: &str, form: &[(&str, &str)]) -> anyhow::Result<(bool, Bytes)> {
        let body = form
            .iter()
            .map(|(key, value)| {
                format!(
                    "{}={}",
                    percent_encoding::utf8_percent_encode(key, percent_encoding::NON_ALPHANUMERIC),
                    percent_encoding::utf8_percent_encode(
                        value,
                        percent_encoding::NON_ALPHANUMERIC
                    )
                )
            })
            .collect::<Vec<_>>()
            .join("&");
        let request = Request::post(url)
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Full::new(Bytes::from(body)))?;
        tokio::time::timeout(Duration::from_secs(30), async {
            let response = self.client.request(request).await?;
            let success = response.status().is_success();
            let body = Limited::new(response.into_body(), 1024 * 1024)
                .collect()
                .await
                .map_err(|error| anyhow!(error))?
                .to_bytes();
            Ok::<_, anyhow::Error>((success, body))
        })
        .await
        .context("Microsoft authentication request timed out")?
    }

    /// Requests a fresh device code and the sign-in instructions for the user.
    pub async fn start(&self) -> anyhow::Result<DeviceCodeResponse> {
        let (success, body) = self
            .post(
                DEVICE_URL,
                &[
                    ("client_id", CLIENT_ID),
                    (
                        "scope",
                        "openid profile offline_access https://graph.microsoft.com/.default",
                    ),
                ],
            )
            .await
            .context("Requesting Microsoft device code")?;
        if !success {
            return Err(oauth_error(&body));
        }
        serde_json::from_slice(&body).context("Parsing device code")
    }

    /// Polls once for a refresh token, returning a retry response for transport failures.
    pub async fn poll(&self, device_code: &str) -> anyhow::Result<TokenPoll> {
        match self
            .post(
                TOKEN_URL,
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", CLIENT_ID),
                    ("device_code", device_code),
                ],
            )
            .await
        {
            Ok((success, body)) => parse_token_response(success, &body),
            Err(error) => {
                tracing::debug!(?error, "polling Microsoft device authorization failed");
                Ok(TokenPoll::Retry)
            }
        }
    }
}

/// Authenticates an O365 collection and saves its encrypted refresh token on success.
///
/// Calls `ready` with the browser URL and user code, then polls until authentication completes,
/// is declined, or expires. Dropping the future cancels pending network work and prevents retries.
/// No state lock is held during network requests or polling delays.
pub async fn monitor_o365_auth(
    state: &EventixState,
    collection: &String,
    ready: impl FnOnce(AuthPrompt),
) -> anyhow::Result<AuthOutcome> {
    {
        let state = state.lock().await;
        let settings = state
            .settings()
            .collections()
            .get(collection)
            .context("Authentication collection no longer exists")?;
        if !matches!(settings.syncer(), SyncerType::O365 { .. }) {
            return Err(anyhow!(
                "Collection '{collection}' is not an O365 collection"
            ));
        }
    }
    let client = MicrosoftDeviceCodeClient::new();
    let device = client.start().await?;
    let lifetime = Duration::from_secs(device.expires_in);
    let deadline = tokio::time::Instant::now()
        .checked_add(lifetime)
        .context("Invalid device code expiry")?;
    ready(AuthPrompt {
        verification_uri: device.verification_uri,
        user_code: device.user_code,
    });
    let result = poll_until_complete(
        deadline,
        Duration::from_secs(device.interval.max(1)),
        || client.poll(&device.device_code),
    )
    .await?;
    match result {
        TokenPoll::Complete(token) => {
            State::store_o365_refresh_token(state, collection, &token).await?;
            Ok(AuthOutcome::Complete)
        }
        TokenPoll::Declined => Ok(AuthOutcome::Declined),
        TokenPoll::Expired => Ok(AuthOutcome::Expired),
        _ => unreachable!("polling returns only terminal outcomes"),
    }
}

/// Waits between requests, respects Microsoft's backoff, and bounds the entire flow by expiry.
async fn poll_until_complete<F: std::future::Future<Output = anyhow::Result<TokenPoll>>>(
    deadline: tokio::time::Instant,
    mut interval: Duration,
    mut poll: impl FnMut() -> F,
) -> anyhow::Result<TokenPoll> {
    loop {
        let next = tokio::time::Instant::now()
            .checked_add(interval)
            .unwrap_or(deadline);
        tokio::time::sleep_until(next.min(deadline)).await;
        if tokio::time::Instant::now() >= deadline {
            return Ok(TokenPoll::Expired);
        }
        let result = match tokio::time::timeout_at(deadline, poll()).await {
            Err(_) => TokenPoll::Expired,
            Ok(result) => result?,
        };
        match result {
            TokenPoll::Pending | TokenPoll::Retry => {}
            TokenPoll::SlowDown => interval = interval.saturating_add(Duration::from_secs(5)),
            terminal => return Ok(terminal),
        }
    }
}

fn parse_token_response(success: bool, body: &[u8]) -> anyhow::Result<TokenPoll> {
    #[derive(Deserialize)]
    struct Token {
        refresh_token: String,
    }
    if success {
        let token: Token = serde_json::from_slice(body).context("Parsing authentication token")?;
        return Ok(TokenPoll::Complete(token.refresh_token));
    }
    let error: OAuthError = serde_json::from_slice(body).context("Parsing OAuth error")?;
    match error.error.as_str() {
        "authorization_pending" => Ok(TokenPoll::Pending),
        "slow_down" => Ok(TokenPoll::SlowDown),
        "authorization_declined" => Ok(TokenPoll::Declined),
        "expired_token" | "bad_verification_code" => Ok(TokenPoll::Expired),
        _ => Err(oauth_error(body)),
    }
}

#[derive(Deserialize)]
struct OAuthError {
    error: String,
    error_description: Option<String>,
}

fn oauth_error(body: &[u8]) -> anyhow::Error {
    match serde_json::from_slice::<OAuthError>(body) {
        Ok(error) => anyhow!(
            "Microsoft OAuth error '{}': {}",
            error.error,
            error.error_description.unwrap_or_default()
        ),
        Err(error) => anyhow!(error).context("Parsing Microsoft OAuth error"),
    }
}

#[cfg(test)]
mod tests {
    use std::{future::ready, time::Duration};
    use tokio::time::Instant;

    use super::{TokenPoll as Poll, parse_token_response as parse_poll, poll_until_complete};

    #[tokio::test(start_paused = true)]
    async fn polling_respects_intervals_backoff_and_transient_failures() {
        let start = Instant::now();
        let mut calls = Vec::new();
        let mut replies = [
            Poll::Pending,
            Poll::SlowDown,
            Poll::Retry,
            Poll::Complete("token".to_string()),
        ]
        .into_iter();
        let result = poll_until_complete(
            start + Duration::from_secs(60),
            Duration::from_secs(5),
            || {
                calls.push(Instant::now().duration_since(start).as_secs());
                ready(Ok(replies.next().unwrap()))
            },
        )
        .await
        .unwrap();
        assert_eq!(calls, [5, 10, 20, 30]);
        assert!(matches!(result, Poll::Complete(token) if token == "token"));
    }

    #[tokio::test(start_paused = true)]
    async fn polling_stops_on_decline_expiry_and_fatal_errors() {
        let result = poll_until_complete(
            Instant::now() + Duration::from_secs(60),
            Duration::from_secs(5),
            || ready(Ok(Poll::Declined)),
        )
        .await
        .unwrap();
        assert!(matches!(result, Poll::Declined));

        let start = Instant::now();
        let mut calls = 0;
        let result = poll_until_complete(
            start + Duration::from_secs(12),
            Duration::from_secs(5),
            || {
                calls += 1;
                ready(Ok(Poll::Pending))
            },
        )
        .await
        .unwrap();
        assert!(matches!(result, Poll::Expired));
        assert_eq!(calls, 2);
        assert_eq!(Instant::now() - start, Duration::from_secs(12));

        let result = poll_until_complete(
            Instant::now() + Duration::from_secs(12),
            Duration::from_secs(5),
            std::future::pending,
        )
        .await
        .unwrap();
        assert!(matches!(result, Poll::Expired));

        assert!(
            poll_until_complete(
                Instant::now() + Duration::from_secs(60),
                Duration::from_secs(5),
                || ready(Err(anyhow::anyhow!("invalid client")))
            )
            .await
            .is_err()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_monitor_stops_polling() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let task = tokio::spawn(poll_until_complete(
            Instant::now() + Duration::from_secs(60),
            Duration::from_secs(5),
            move || {
                observed.fetch_add(1, Ordering::SeqCst);
                ready(Ok(Poll::Pending))
            },
        ));
        tokio::task::yield_now().await;
        task.abort();
        assert!(task.await.err().unwrap().is_cancelled());
        tokio::time::advance(Duration::from_secs(60)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn handles_device_authorization_results() {
        for (error, expected) in [
            ("authorization_pending", "pending"),
            ("slow_down", "slow"),
            ("authorization_declined", "declined"),
            ("expired_token", "expired"),
            ("bad_verification_code", "expired"),
        ] {
            let body = format!(r#"{{"error":"{error}"}}"#);
            let result = match parse_poll(false, body.as_bytes()).unwrap() {
                Poll::Pending => "pending",
                Poll::SlowDown => "slow",
                Poll::Declined => "declined",
                Poll::Expired => "expired",
                Poll::Complete(_) => panic!("unexpected token"),
                Poll::Retry => panic!("unexpected transport retry"),
            };
            assert_eq!(result, expected);
        }
        assert!(
            matches!(parse_poll(true, br#"{"refresh_token":"secret"}"#).unwrap(),
            Poll::Complete(token) if token == "secret")
        );
        let error = parse_poll(
            false,
            br#"{"error":"invalid_client","error_description":"bad client"}"#,
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("bad client"));
        assert!(parse_poll(true, b"invalid").is_err());
    }
}
