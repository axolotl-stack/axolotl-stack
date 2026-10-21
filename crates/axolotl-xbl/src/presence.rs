//! Xbox Live Presence API.
//! Wire behavior: df-mc/go-xsapi presence/client.go:Update, update, remove, heartbeatAfter.

use crate::auth::{XblToken, sign_service_request, update_server_time_from_header};
use crate::constants::endpoints;
use crate::error::{XblError, XblResult};
use serde::Serialize;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceState {
    Active,
    Inactive,
}

#[derive(Debug, Serialize)]
pub struct PresenceUpdate {
    state: &'static str,
}

pub struct PresenceClient {
    client: reqwest::Client,
    // A removal invalidates pending retries without holding the lock over sleeps.
    generation: Mutex<u64>,
}

fn heartbeat_after(response: &reqwest::Response) -> u64 {
    response
        .headers()
        .get("X-Heartbeat-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v <= (i64::MAX as u64) / 1_000_000_000)
        .unwrap_or(0)
}

fn retry_after(response: &reqwest::Response) -> Duration {
    let value = response
        .headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok());
    let seconds = value
        .and_then(|v| v.parse::<u64>().ok())
        .or_else(|| {
            let date = chrono::DateTime::parse_from_rfc2822(value?).ok()?;
            u64::try_from((date.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds())
                .ok()
        })
        .filter(|v| *v > 0 && *v <= (i64::MAX as u64) / 1_000_000_000)
        .unwrap_or(5);
    Duration::from_secs(seconds)
}

impl PresenceClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            generation: Mutex::new(0),
        }
    }

    /// Returns the server's heartbeat delay in seconds, or zero if absent/invalid.
    /// Callers must choose their own scheduling fallback for zero.
    pub async fn update(&self, token: &XblToken, state: PresenceState) -> XblResult<u64> {
        let url = endpoints::USER_PRESENCE_FMT.replace("{}", &token.xuid);
        let generation = *self.generation.lock().await;
        for attempt in 0..3 {
            let mut current = self.generation.lock().await;
            if state == PresenceState::Inactive {
                *current = current.wrapping_add(1);
            } else if *current != generation {
                return Err(XblError::XboxLive(
                    "Presence update stopped by removal".into(),
                ));
            }
            let body = if state == PresenceState::Active {
                serde_json::to_vec(&PresenceUpdate { state: "active" })?
            } else {
                Vec::new()
            };
            let mut request = self
                .client
                .request(
                    if state == PresenceState::Active {
                        reqwest::Method::POST
                    } else {
                        reqwest::Method::DELETE
                    },
                    &url,
                )
                .header("Content-Type", "application/json")
                .header("Cache-Control", "no-cache")
                .header("Accept-Language", "en-US,en")
                .header("x-xbl-contract-version", "3")
                .body(body)
                .build()?;
            sign_service_request(&mut request, token).await?;
            let response = self.client.execute(request).await?;
            if let Some(date) = response.headers().get("Date").and_then(|v| v.to_str().ok()) {
                update_server_time_from_header(date);
            }
            let status = response.status();
            if status == reqwest::StatusCode::OK
                || (state == PresenceState::Active && status == reqwest::StatusCode::CREATED)
            {
                return Ok(heartbeat_after(&response));
            }
            let error = XblError::HttpStatus {
                status,
                operation: "update presence",
            };
            if state == PresenceState::Inactive
                || attempt == 2
                || !matches!(status.as_u16(), 429 | 503)
            {
                return Err(error);
            }
            let delay = retry_after(&response);
            drop(response);
            drop(current);
            tokio::time::sleep(delay).await;
        }
        Err(XblError::XboxLive(
            "Presence retry attempts exhausted".into(),
        ))
    }

    pub async fn set_active(&self, token: &XblToken) -> XblResult<u64> {
        self.update(token, PresenceState::Active).await
    }

    pub async fn set_inactive(&self, token: &XblToken) -> XblResult<u64> {
        self.update(token, PresenceState::Inactive).await
    }
}

impl Default for PresenceClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_invalid_and_zero_heartbeat_are_zero() {
        for value in [
            None,
            Some("invalid"),
            Some("-1"),
            Some("0"),
            Some("18446744073709551615"),
            Some("300"),
        ] {
            let mut response = tokio_tungstenite::tungstenite::http::Response::builder();
            if let Some(value) = value {
                response = response.header("X-Heartbeat-After", value);
            }
            let response: reqwest::Response = response.body("").unwrap().into();
            assert_eq!(
                heartbeat_after(&response),
                if value == Some("300") { 300 } else { 0 }
            );
        }
    }

    #[test]
    fn retry_delay_rejects_unrepresentable_durations() {
        let response: reqwest::Response = tokio_tungstenite::tungstenite::http::Response::builder()
            .header("Retry-After", "18446744073709551615")
            .body("")
            .unwrap()
            .into();
        assert_eq!(retry_after(&response), Duration::from_secs(5));
    }
}
