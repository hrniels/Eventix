// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use crate::api::{HTMLResponse, JsonError};
use crate::html::filters;
use crate::http::HttpClient;
use anyhow::{Context, anyhow};
use askama::Template;
use async_trait::async_trait;
use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::{get, post},
};
use eventix_locale::Locale;
use eventix_state::{
    EncryptedPassword, EventixState, State as AppState, SyncerType, decrypt_password,
    encrypt_password, retrieve_portal_secret,
};
use formatx::formatx;
use serde::{Deserialize, Serialize};

const CLIENT_ID: &str = "d3590ed6-52b3-4102-aeff-aad2292ab01c";
const OAUTH_SCOPE: &str = "openid profile offline_access https://graph.microsoft.com/.default";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_poll_interval")]
    interval: u64,
}

fn default_poll_interval() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    error: String,
    error_description: Option<String>,
}

enum TokenPoll {
    Pending,
    Retry,
    SlowDown,
    Complete(String),
    Declined,
    Expired,
}

#[async_trait]
trait DeviceCodeClient: Send + Sync {
    /// Starts a device authorization and returns the information safe to present to the user.
    async fn start(&self) -> anyhow::Result<DeviceCodeResponse>;

    /// Advances an existing device authorization by one Microsoft token request.
    async fn poll(&self, device_code: &str) -> anyhow::Result<TokenPoll>;
}

struct MicrosoftDeviceCodeClient {
    client: HttpClient,
}

impl MicrosoftDeviceCodeClient {
    fn new() -> Self {
        // Reuse the HTTPS connection across the initial request and subsequent token polls.
        Self {
            client: HttpClient::new(),
        }
    }
}

#[async_trait]
impl DeviceCodeClient for MicrosoftDeviceCodeClient {
    async fn start(&self) -> anyhow::Result<DeviceCodeResponse> {
        let response = self
            .client
            .post_form(
                DEVICE_CODE_URL,
                &[("client_id", CLIENT_ID), ("scope", OAUTH_SCOPE)],
                Duration::from_secs(30),
            )
            .await
            .context("requesting Microsoft device code")?;
        if response.status.is_success() {
            serde_json::from_slice(&response.body).context("parsing device code response")
        } else {
            // OAuth error responses are structured JSON and contain the useful failure reason.
            let error: OAuthError =
                serde_json::from_slice(&response.body).context("parsing device code error")?;
            Err(oauth_error(error))
        }
    }

    async fn poll(&self, device_code: &str) -> anyhow::Result<TokenPoll> {
        let response = match self
            .client
            .post_form(
                TOKEN_URL,
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", CLIENT_ID),
                    ("device_code", device_code),
                ],
                Duration::from_secs(30),
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::debug!(?error, "polling Microsoft device authorization failed");
                return Ok(TokenPoll::Retry);
            }
        };
        parse_token_response(response.status.is_success(), &response.body)
    }
}

fn parse_token_response(success: bool, body: &[u8]) -> anyhow::Result<TokenPoll> {
    if success {
        let token: TokenResponse =
            serde_json::from_slice(body).context("parsing token response")?;
        return Ok(TokenPoll::Complete(token.refresh_token));
    }

    let error: OAuthError = serde_json::from_slice(body).context("parsing token error")?;
    match error.error.as_str() {
        "authorization_pending" => Ok(TokenPoll::Pending),
        "slow_down" => Ok(TokenPoll::SlowDown),
        "authorization_declined" => Ok(TokenPoll::Declined),
        "expired_token" | "bad_verification_code" => Ok(TokenPoll::Expired),
        _ => Err(oauth_error(error)),
    }
}

fn oauth_error(error: OAuthError) -> anyhow::Error {
    anyhow!(
        "Microsoft OAuth error '{}': {}",
        error.error,
        error.error_description.unwrap_or_default()
    )
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PendingAuthState {
    col_id: String,
    // The Microsoft device code is a credential and therefore never leaves the server in plaintext.
    device_code: String,
    expires_at_ms: u64,
    interval_ms: u64,
    next_poll_at_ms: u64,
}

struct DeviceAuthService {
    client: Arc<dyn DeviceCodeClient>,
}

impl DeviceAuthService {
    fn new(client: Arc<dyn DeviceCodeClient>) -> Self {
        Self { client }
    }
}

fn current_time_ms() -> anyhow::Result<u64> {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("system time is before Unix epoch")?
        .as_millis()
        .try_into()
        .context("system time does not fit into milliseconds")
}

fn seal_auth_state(secret: &[u8], state: &PendingAuthState) -> anyhow::Result<EncryptedPassword> {
    let plaintext =
        serde_json::to_string(state).context("serializing device authorization state")?;
    encrypt_password(secret, &plaintext).context("encrypting device authorization state")
}

fn open_auth_state(secret: &[u8], encrypted: &EncryptedPassword) -> Option<PendingAuthState> {
    let plaintext = decrypt_password(secret, encrypted).ok()?;
    serde_json::from_str(&plaintext).ok()
}

pub fn router(state: EventixState) -> Router {
    let service = Arc::new(DeviceAuthService::new(Arc::new(
        MicrosoftDeviceCodeClient::new(),
    )));
    Router::new()
        .route("/auth", get(handler))
        .route("/auth/poll", post(poll_handler))
        .layer(Extension(service))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct Request {
    calendar: String,
    op_url: String,
    spinner_id: String,
}

#[derive(Template)]
#[template(path = "ajax/auth.htm")]
struct AuthTemplate {
    locale: Arc<dyn Locale + Send + Sync>,
    error: String,
    auth_nonce: String,
    auth_ciphertext: String,
    verification_uri: String,
    user_code: String,
    interval_ms: u64,
    op_url: String,
    spinner_id: String,
}

async fn handler(
    State(state): State<EventixState>,
    Extension(service): Extension<Arc<DeviceAuthService>>,
    Query(req): Query<Request>,
) -> Result<impl IntoResponse, JsonError> {
    let locale = {
        let state = state.lock().await;
        let collection = state
            .settings()
            .collections()
            .get(&req.calendar)
            .ok_or_else(|| anyhow!("No collection with id {}", req.calendar))?;
        if !matches!(collection.syncer(), SyncerType::O365 { .. }) {
            return Err(anyhow!("Collection '{}' is not an O365 collection", req.calendar).into());
        }
        state.locale()
    };

    // Do not hold the application-state lock while waiting on Microsoft.
    let device = service.client.start().await?;
    let interval_ms = device
        .interval
        .max(1)
        .checked_mul(1000)
        .context("device authorization interval is too large")?;
    let expires_in_ms = device
        .expires_in
        .checked_mul(1000)
        .context("device authorization lifetime is too large")?;
    let now_ms = current_time_ms()?;
    let auth_state = PendingAuthState {
        col_id: req.calendar.clone(),
        device_code: device.device_code,
        expires_at_ms: now_ms
            .checked_add(expires_in_ms)
            .context("device authorization expiry is too large")?,
        interval_ms,
        next_poll_at_ms: now_ms
            .checked_add(interval_ms)
            .context("device authorization interval is too large")?,
    };
    let secret = retrieve_portal_secret()
        .await
        .context("retrieving portal secret")?;
    let auth_state = seal_auth_state(&secret, &auth_state)?;

    let error = formatx!(locale.translate("error.reauth_required"), &req.calendar).unwrap();
    let html = AuthTemplate {
        locale,
        error,
        auth_nonce: auth_state.nonce,
        auth_ciphertext: auth_state.ciphertext,
        verification_uri: device.verification_uri,
        user_code: device.user_code,
        interval_ms,
        op_url: req.op_url,
        spinner_id: req.spinner_id,
    }
    .render()
    .context("auth template")?;

    Ok(Json(HTMLResponse::new(html)))
}

#[derive(Debug, Deserialize)]
struct PollRequest {
    auth_nonce: String,
    auth_ciphertext: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PollResponse {
    Pending {
        auth_nonce: String,
        auth_ciphertext: String,
        retry_after_ms: u64,
    },
    Complete,
    Declined,
    Expired,
}

async fn poll_handler(
    State(state): State<EventixState>,
    Extension(service): Extension<Arc<DeviceAuthService>>,
    Json(req): Json<PollRequest>,
) -> Result<impl IntoResponse, JsonError> {
    let secret = retrieve_portal_secret()
        .await
        .context("retrieving portal secret")?;
    let encrypted = EncryptedPassword {
        nonce: req.auth_nonce,
        ciphertext: req.auth_ciphertext,
    };
    let Some(mut auth_state) = open_auth_state(&secret, &encrypted) else {
        return Ok(Json(PollResponse::Expired));
    };
    let now_ms = current_time_ms()?;
    if now_ms >= auth_state.expires_at_ms {
        return Ok(Json(PollResponse::Expired));
    }
    if now_ms < auth_state.next_poll_at_ms {
        return Ok(Json(PollResponse::Pending {
            auth_nonce: encrypted.nonce,
            auth_ciphertext: encrypted.ciphertext,
            retry_after_ms: auth_state.next_poll_at_ms - now_ms,
        }));
    }

    let poll = service.client.poll(&auth_state.device_code).await?;

    match poll {
        TokenPoll::Complete(token) => {
            AppState::store_o365_refresh_token(&state, &auth_state.col_id, &token).await?;
            Ok(Json(PollResponse::Complete))
        }
        TokenPoll::Pending | TokenPoll::Retry => {
            let now_ms = current_time_ms()?;
            if now_ms >= auth_state.expires_at_ms {
                return Ok(Json(PollResponse::Expired));
            }
            auth_state.next_poll_at_ms = now_ms.saturating_add(auth_state.interval_ms);
            let encrypted = seal_auth_state(&secret, &auth_state)?;
            Ok(Json(PollResponse::Pending {
                auth_nonce: encrypted.nonce,
                auth_ciphertext: encrypted.ciphertext,
                retry_after_ms: auth_state.interval_ms,
            }))
        }
        TokenPoll::SlowDown => {
            let now_ms = current_time_ms()?;
            if now_ms >= auth_state.expires_at_ms {
                return Ok(Json(PollResponse::Expired));
            }
            // RFC 8628 requires increasing the interval by five seconds after `slow_down`.
            auth_state.interval_ms = auth_state.interval_ms.saturating_add(5000);
            auth_state.next_poll_at_ms = now_ms.saturating_add(auth_state.interval_ms);
            let encrypted = seal_auth_state(&secret, &auth_state)?;
            Ok(Json(PollResponse::Pending {
                auth_nonce: encrypted.nonce,
                auth_ciphertext: encrypted.ciphertext,
                retry_after_ms: auth_state.interval_ms,
            }))
        }
        TokenPoll::Declined => Ok(Json(PollResponse::Declined)),
        TokenPoll::Expired => Ok(Json(PollResponse::Expired)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EncryptedPassword, PendingAuthState, TokenPoll, open_auth_state, parse_token_response,
        seal_auth_state,
    };

    #[test]
    fn sealed_auth_state_round_trips_without_exposing_device_code() {
        let state = PendingAuthState {
            col_id: "collection".to_string(),
            device_code: "sensitive-device-code".to_string(),
            expires_at_ms: 20_000,
            interval_ms: 5_000,
            next_poll_at_ms: 10_000,
        };

        let encrypted = seal_auth_state(b"secret", &state).unwrap();

        assert!(!encrypted.ciphertext.contains(&state.device_code));
        assert_eq!(open_auth_state(b"secret", &encrypted), Some(state));
    }

    #[test]
    fn rejects_tampered_auth_state() {
        let state = PendingAuthState {
            col_id: "collection".to_string(),
            device_code: "device-code".to_string(),
            expires_at_ms: 20_000,
            interval_ms: 5_000,
            next_poll_at_ms: 10_000,
        };
        let encrypted = seal_auth_state(b"secret", &state).unwrap();

        assert_eq!(open_auth_state(b"other-secret", &encrypted), None);
        assert_eq!(
            open_auth_state(
                b"secret",
                &EncryptedPassword {
                    nonce: "invalid".to_string(),
                    ciphertext: "invalid".to_string(),
                },
            ),
            None
        );
    }

    #[test]
    fn parses_successful_token_response() {
        let response = parse_token_response(true, br#"{"refresh_token":"secret"}"#).unwrap();
        let TokenPoll::Complete(token) = response else {
            panic!("expected completed token response");
        };
        assert_eq!(token, "secret");
    }

    #[test]
    fn parses_expected_polling_errors() {
        for (error, expected) in [
            ("authorization_pending", "pending"),
            ("slow_down", "slow_down"),
            ("authorization_declined", "declined"),
            ("expired_token", "expired"),
            ("bad_verification_code", "expired"),
        ] {
            let body = format!(r#"{{"error":"{error}"}}"#);
            let response = parse_token_response(false, body.as_bytes()).unwrap();
            let actual = match response {
                TokenPoll::Pending => "pending",
                TokenPoll::Retry => "retry",
                TokenPoll::SlowDown => "slow_down",
                TokenPoll::Declined => "declined",
                TokenPoll::Expired => "expired",
                TokenPoll::Complete(_) => "complete",
            };
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn rejects_unexpected_oauth_error() {
        let error = parse_token_response(
            false,
            br#"{"error":"invalid_client","error_description":"bad client"}"#,
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("invalid_client"));
        assert!(error.to_string().contains("bad client"));
    }
}
