// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

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
use eventix_state::{EventixState, State as AppState, SyncerType};
use formatx::formatx;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::api::{HTMLResponse, JsonError};
use crate::html::filters;
use crate::http::HttpClient;

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
        let response = self
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
            .context("polling Microsoft device authorization")?;
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

struct PendingAuth {
    col_id: String,
    // The Microsoft device code is a credential and therefore never leaves the server.
    device_code: String,
    expires_at: Instant,
    interval: Duration,
    next_poll: Instant,
    polling: bool,
}

struct DeviceAuthService {
    client: Arc<dyn DeviceCodeClient>,
    // The browser only receives the opaque UUID used to address these server-side sessions.
    sessions: Mutex<HashMap<Uuid, PendingAuth>>,
}

impl DeviceAuthService {
    fn new(client: Arc<dyn DeviceCodeClient>) -> Self {
        Self {
            client,
            sessions: Mutex::new(HashMap::new()),
        }
    }
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
    auth_id: String,
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
    let auth_id = Uuid::new_v4();
    let interval = Duration::from_secs(device.interval.max(1));
    let now = Instant::now();
    service.sessions.lock().await.insert(
        auth_id,
        PendingAuth {
            col_id: req.calendar.clone(),
            device_code: device.device_code,
            expires_at: now + Duration::from_secs(device.expires_in),
            interval,
            next_poll: now + interval,
            polling: false,
        },
    );

    let error = formatx!(locale.translate("error.reauth_required"), &req.calendar).unwrap();
    let html = AuthTemplate {
        locale,
        error,
        auth_id: auth_id.to_string(),
        verification_uri: device.verification_uri,
        user_code: device.user_code,
        interval_ms: interval.as_millis() as u64,
        op_url: req.op_url,
        spinner_id: req.spinner_id,
    }
    .render()
    .context("auth template")?;

    Ok(Json(HTMLResponse::new(html)))
}

#[derive(Debug, Deserialize)]
struct PollRequest {
    auth_id: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PollResponse {
    Pending { retry_after_ms: u64 },
    Complete,
    Declined,
    Expired,
}

async fn poll_handler(
    State(state): State<EventixState>,
    Extension(service): Extension<Arc<DeviceAuthService>>,
    Query(req): Query<PollRequest>,
) -> Result<impl IntoResponse, JsonError> {
    let auth_id = Uuid::parse_str(&req.auth_id).context("invalid authorization id")?;
    let now = Instant::now();
    let (device_code, col_id) = {
        let mut sessions = service.sessions.lock().await;
        let Some(session) = sessions.get_mut(&auth_id) else {
            return Ok(Json(PollResponse::Expired));
        };
        if now >= session.expires_at {
            sessions.remove(&auth_id);
            return Ok(Json(PollResponse::Expired));
        }
        if session.polling || now < session.next_poll {
            let retry_after = session.next_poll.saturating_duration_since(now);
            return Ok(Json(PollResponse::Pending {
                retry_after_ms: retry_after.as_millis().max(1) as u64,
            }));
        }
        // Mark the session before releasing the mutex so concurrent browser requests cannot
        // issue overlapping token polls for the same device code.
        session.polling = true;
        (session.device_code.clone(), session.col_id.clone())
    };

    // The session mutex must not be held across the network request; other authorizations should
    // remain responsive while Microsoft handles this poll.
    let poll = match service.client.poll(&device_code).await {
        Ok(poll) => poll,
        Err(error) => {
            // Transport failures are retryable by reopening the popup, so leave the session
            // available rather than converting the failure into a terminal OAuth state.
            if let Some(session) = service.sessions.lock().await.get_mut(&auth_id) {
                session.polling = false;
            }
            return Err(error.into());
        }
    };

    match poll {
        TokenPoll::Complete(token) => {
            AppState::store_o365_refresh_token(&state, &col_id, &token).await?;
            service.sessions.lock().await.remove(&auth_id);
            Ok(Json(PollResponse::Complete))
        }
        TokenPoll::Pending => {
            let mut sessions = service.sessions.lock().await;
            let Some(session) = sessions.get_mut(&auth_id) else {
                return Ok(Json(PollResponse::Expired));
            };
            session.polling = false;
            session.next_poll = Instant::now() + session.interval;
            Ok(Json(PollResponse::Pending {
                retry_after_ms: session.interval.as_millis() as u64,
            }))
        }
        TokenPoll::SlowDown => {
            let mut sessions = service.sessions.lock().await;
            let Some(session) = sessions.get_mut(&auth_id) else {
                return Ok(Json(PollResponse::Expired));
            };
            session.polling = false;
            // RFC 8628 requires increasing the interval by five seconds after `slow_down`.
            session.interval += Duration::from_secs(5);
            session.next_poll = Instant::now() + session.interval;
            Ok(Json(PollResponse::Pending {
                retry_after_ms: session.interval.as_millis() as u64,
            }))
        }
        TokenPoll::Declined => {
            service.sessions.lock().await.remove(&auth_id);
            Ok(Json(PollResponse::Declined))
        }
        TokenPoll::Expired => {
            service.sessions.lock().await.remove(&auth_id);
            Ok(Json(PollResponse::Expired))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TokenPoll, parse_token_response};

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
