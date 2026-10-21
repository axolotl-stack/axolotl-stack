//! Windows Live OAuth device-code flow.
//! Source: go-xsapi/xal/sisu/oauth2.go:DeviceAuth, DeviceAccessToken, tokenRefresher.Token.

use super::{LIVE_CONNECT_URL, LIVE_TOKEN_URL, MINECRAFT_CLIENT_ID, XAL_USER_AGENT, XBL_SCOPE};
use crate::error::{XblError, XblResult, response_json};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct DeviceCodeAuth {
    client: reqwest::Client,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

impl std::fmt::Debug for DeviceCodeResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCodeResponse")
            .field("codes", &"[REDACTED]")
            .field("interval", &self.interval)
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub token_type: String,
    #[serde(default)]
    pub refresh_token: String,
    pub expires_in: u64,
    pub user_id: Option<String>,
}

impl std::fmt::Debug for OAuthToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthToken")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct PollResponse {
    error: Option<String>,
    access_token: Option<String>,
    token_type: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    user_id: Option<String>,
}

impl DeviceCodeAuth {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("valid HTTP client configuration"),
        }
    }
    pub async fn start(&self) -> XblResult<DeviceCodeResponse> {
        self.start_at(LIVE_CONNECT_URL).await
    }
    async fn start_at(&self, url: &str) -> XblResult<DeviceCodeResponse> {
        let response = self
            .client
            .post(url)
            .form(&[
                ("client_id", MINECRAFT_CLIENT_ID),
                ("scope", XBL_SCOPE),
                ("response_type", "device_code"),
            ])
            .send()
            .await?;
        let code: DeviceCodeResponse =
            response_json(response, "OAuth device authorization").await?;
        if code.device_code.is_empty() || code.user_code.is_empty() || code.expires_in == 0 {
            return Err(XblError::Auth(
                "Invalid OAuth device authorization response".into(),
            ));
        }
        Ok(code)
    }
    pub async fn poll(&self, device_code: &str) -> XblResult<Option<OAuthToken>> {
        self.poll_at(LIVE_TOKEN_URL, device_code).await
    }
    async fn poll_at(&self, url: &str, device_code: &str) -> XblResult<Option<OAuthToken>> {
        let response = self
            .client
            .post(url)
            .form(&[
                ("client_id", MINECRAFT_CLIENT_ID),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code),
            ])
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        let poll: PollResponse = match serde_json::from_slice(&bytes) {
            Ok(poll) => poll,
            Err(_) if !status.is_success() => {
                return Err(XblError::HttpStatus {
                    status,
                    operation: "OAuth device token",
                });
            }
            Err(error) => return Err(error.into()),
        };
        // Pending/slow_down are the protocol's expected HTTP 400 control responses,
        // never accept a token-shaped error body as a successful authentication.
        if !status.is_success() {
            if status == reqwest::StatusCode::BAD_REQUEST {
                match poll.error.as_deref() {
                    Some("authorization_pending") => return Ok(None),
                    Some("slow_down") => return Err(XblError::AuthorizationSlowDown),
                    Some("expired_token") => return Err(XblError::DeviceCodeExpired),
                    _ => {}
                }
            }
            return Err(XblError::HttpStatus {
                status,
                operation: "OAuth device token",
            });
        }
        if poll.error.is_some() {
            return Err(XblError::Auth("OAuth device authorization rejected".into()));
        }
        let token = OAuthToken {
            access_token: poll
                .access_token
                .ok_or_else(|| XblError::Auth("Missing OAuth access token".into()))?,
            refresh_token: poll
                .refresh_token
                .ok_or_else(|| XblError::Auth("Missing OAuth refresh token".into()))?,
            token_type: poll.token_type.unwrap_or_else(|| "Bearer".into()),
            expires_in: poll.expires_in.unwrap_or(0),
            user_id: poll.user_id,
        };
        validate_token(&token)?;
        Ok(Some(token))
    }
    pub async fn wait_for_auth(&self, code: &DeviceCodeResponse) -> XblResult<OAuthToken> {
        let mut interval = Duration::from_secs(if code.interval == 0 { 5 } else { code.interval });
        let deadline = tokio::time::Instant::now()
            .checked_add(Duration::from_secs(code.expires_in))
            .ok_or_else(|| XblError::Auth("Invalid OAuth device-code lifetime".into()))?;
        loop {
            if tokio::time::Instant::now()
                .checked_add(interval)
                .is_none_or(|next| next >= deadline)
            {
                return Err(XblError::DeviceCodeExpired);
            }
            tokio::time::sleep(interval).await;
            match tokio::time::timeout_at(deadline, self.poll(&code.device_code)).await {
                Err(_) => return Err(XblError::DeviceCodeExpired),
                Ok(Ok(Some(token))) => return Ok(token),
                Ok(Ok(None)) => {}
                Ok(Err(XblError::AuthorizationSlowDown)) => {
                    interval = interval.saturating_add(Duration::from_secs(5))
                }
                Ok(Err(error)) => return Err(error),
            }
        }
    }
    pub async fn refresh(&self, refresh_token: &str) -> XblResult<OAuthToken> {
        self.refresh_at(LIVE_TOKEN_URL, refresh_token).await
    }
    async fn refresh_at(&self, url: &str, refresh_token: &str) -> XblResult<OAuthToken> {
        if refresh_token.is_empty() {
            return Err(XblError::Auth("Missing OAuth refresh token".into()));
        }
        let response = self
            .client
            .post(url)
            .header("User-Agent", XAL_USER_AGENT)
            .form(&[
                ("client_id", MINECRAFT_CLIENT_ID),
                ("scope", XBL_SCOPE),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await?;
        let mut token: OAuthToken = response_json(response, "OAuth token refresh").await?;
        // oauth2 reuseTokenSource retains the old refresh token when a provider omits a replacement.
        if token.refresh_token.is_empty() {
            token.refresh_token = refresh_token.into();
        }
        validate_token(&token)?;
        Ok(token)
    }
}
fn validate_token(token: &OAuthToken) -> XblResult<()> {
    if token.access_token.is_empty()
        || token.expires_in == 0
        || std::time::Instant::now()
            .checked_add(Duration::from_secs(token.expires_in))
            .is_none()
    {
        return Err(XblError::Auth("Invalid OAuth token response".into()));
    }
    Ok(())
}
impl Default for DeviceCodeAuth {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn server(status: u16, body: &str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 8192];
            assert_ne!(socket.read(&mut buffer).await.unwrap(), 0);
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        url
    }
    #[tokio::test]
    async fn non_success_token_body_is_not_accepted() {
        let url=server(500,r#"{"access_token":"secret-access","refresh_token":"secret-refresh","expires_in":3600}"#).await;
        let error = DeviceCodeAuth::new()
            .poll_at(&url, "device-secret")
            .await
            .unwrap_err();
        assert_eq!(
            error.status(),
            Some(reqwest::StatusCode::INTERNAL_SERVER_ERROR)
        );
        assert!(!format!("{error} {error:?}").contains("secret"));
    }
    #[tokio::test]
    async fn oauth_failure_description_cannot_leak_token() {
        let url = server(
            400,
            r#"{"error":"bad_request","error_description":"secret-access"}"#,
        )
        .await;
        let error = DeviceCodeAuth::new()
            .poll_at(&url, "device-secret")
            .await
            .unwrap_err();
        assert_eq!(error.status(), Some(reqwest::StatusCode::BAD_REQUEST));
        assert!(!format!("{error} {error:?}").contains("secret"));
    }
    #[tokio::test]
    async fn pending_and_slow_down_are_protocol_control_responses() {
        let auth = DeviceCodeAuth::new();
        let pending = server(400, r#"{"error":"authorization_pending"}"#).await;
        assert!(auth.poll_at(&pending, "code").await.unwrap().is_none());
        let slow = server(400, r#"{"error":"slow_down"}"#).await;
        assert!(matches!(
            auth.poll_at(&slow, "code").await,
            Err(XblError::AuthorizationSlowDown)
        ));
    }
    #[tokio::test]
    async fn malformed_success_response_is_redacted() {
        let url = server(
            200,
            r#"{"access_token":{},"refresh_token":"secret-refresh","expires_in":"secret-expiry"}"#,
        )
        .await;
        let error = DeviceCodeAuth::new()
            .poll_at(&url, "code")
            .await
            .unwrap_err();
        assert!(!format!("{error} {error:?}").contains("secret"));
    }
    #[tokio::test]
    async fn overflowing_oauth_lifetime_is_rejected() {
        let url=server(200,r#"{"access_token":"secret-access","refresh_token":"secret-refresh","expires_in":18446744073709551615}"#).await;
        let error = DeviceCodeAuth::new()
            .poll_at(&url, "code")
            .await
            .unwrap_err();
        assert!(!format!("{error} {error:?}").contains("secret"));
    }
    #[tokio::test]
    async fn expired_device_code_does_not_poll_network() {
        let code = DeviceCodeResponse {
            device_code: "code".into(),
            user_code: "user".into(),
            verification_uri: "uri".into(),
            interval: 5,
            expires_in: 0,
        };
        assert!(matches!(
            DeviceCodeAuth::new().wait_for_auth(&code).await,
            Err(XblError::DeviceCodeExpired)
        ));
    }
}
