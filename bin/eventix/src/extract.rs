use axum::body::{self, Body};
use axum::extract::FromRequest;
use axum::http::Request;
use serde::de::DeserializeOwned;

#[derive(Debug)]
pub struct MultiForm<T>(pub T);

#[axum::async_trait]
impl<T, S> FromRequest<S> for MultiForm<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = String;

    async fn from_request(req: Request<Body>, _state: &S) -> Result<Self, Self::Rejection> {
        let body = body::to_bytes(req.into_body(), 32 * 1024)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self(
            // disable strict-mode to support array fields
            serde_qs::Config::new(5, false)
                .deserialize_bytes(&body)
                .map_err(|e| e.to_string())?,
        ))
    }
}
