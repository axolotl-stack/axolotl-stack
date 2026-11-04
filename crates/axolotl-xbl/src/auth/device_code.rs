//! Windows Live OAuth device-code flow.
//! Source: go-xsapi/xal/sisu/oauth2.go:DeviceAuth, DeviceAccessToken, tokenRefresher.Token.

use super::http::LazyClient;
use super::{LIVE_CONNECT_URL, LIVE_TOKEN_URL, MINECRAFT_CLIENT_ID, XAL_USER_AGENT, XBL_SCOPE};
use crate::error::{XblError, XblResult, response_json};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Windows Live device-code login. Construction does no I/O and cannot fail;
/// the HTTP client is built on the first request.
pub struct DeviceCodeAuth {
    client: LazyClient,
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
            client: LazyClient::new(Duration::from_secs(15)),
        }
    }
    pub async fn start(&self) -> XblResult<DeviceCodeResponse> {
        self.start_at(LIVE_CONNECT_URL).await
    }
    async fn start_at(&self, url: &str) -> XblResult<DeviceCodeResponse> {
        let response = self
            .client
            .get()?
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
            .get()?
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
        poll_until_authorized(code, || self.poll(&code.device_code)).await
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
            .get()?
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
/// Polls until the user authorizes: every `interval` seconds (5 when the
/// server sends 0), five seconds longer after each `slow_down` (RFC 8628
/// section 3.5), and never past the device code's lifetime.
async fn poll_until_authorized<F, Fut>(
    code: &DeviceCodeResponse,
    mut poll: F,
) -> XblResult<OAuthToken>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = XblResult<Option<OAuthToken>>>,
{
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
        match tokio::time::timeout_at(deadline, poll()).await {
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
    /// A refresh response without `refresh_token` keeps the one that was
    /// sent; a response with one replaces it.
    #[tokio::test]
    async fn refresh_keeps_the_refresh_token_unless_replaced() {
        let auth = DeviceCodeAuth::new();
        let omitted = server(
            200,
            r#"{"access_token":"a","token_type":"bearer","expires_in":3600,"user_id":null}"#,
        )
        .await;
        let token = auth.refresh_at(&omitted, "old-refresh").await.unwrap();
        assert_eq!(token.refresh_token, "old-refresh");
        assert_eq!(token.access_token, "a");

        let replaced = server(
            200,
            r#"{"access_token":"b","token_type":"bearer","refresh_token":"new-refresh","expires_in":3600,"user_id":null}"#,
        )
        .await;
        let token = auth.refresh_at(&replaced, "old-refresh").await.unwrap();
        assert_eq!(token.refresh_token, "new-refresh");

        assert!(auth.refresh_at(&replaced, "").await.is_err());
    }

    /// Each `slow_down` lengthens the poll interval by five seconds for the
    /// rest of the wait; `authorization_pending` keeps it.
    #[tokio::test(start_paused = true)]
    async fn slow_down_backs_off_by_five_seconds_each_time() {
        let code = DeviceCodeResponse {
            device_code: "code".into(),
            user_code: "user".into(),
            verification_uri: "uri".into(),
            interval: 5,
            expires_in: 900,
        };
        let start = tokio::time::Instant::now();
        let polls = std::cell::RefCell::new(Vec::new());
        let token = poll_until_authorized(&code, || {
            let mut polls = polls.borrow_mut();
            polls.push(start.elapsed().as_secs());
            let answer = match polls.len() {
                1 | 2 => Err(XblError::AuthorizationSlowDown),
                3 => Ok(None),
                _ => Ok(Some(OAuthToken {
                    access_token: "a".into(),
                    token_type: "bearer".into(),
                    refresh_token: "r".into(),
                    expires_in: 60,
                    user_id: None,
                })),
            };
            async move { answer }
        })
        .await
        .unwrap();
        assert_eq!(token.access_token, "a");
        // 5 s, then +10 s, +15 s after two slow_downs, +15 s after pending.
        assert_eq!(*polls.borrow(), [5, 15, 30, 45]);
    }

    /// Back-off never polls past the device code's lifetime.
    #[tokio::test(start_paused = true)]
    async fn slow_down_stops_at_the_device_code_lifetime() {
        let code = DeviceCodeResponse {
            device_code: "code".into(),
            user_code: "user".into(),
            verification_uri: "uri".into(),
            interval: 5,
            expires_in: 20,
        };
        let polls = std::cell::Cell::new(0);
        let result = poll_until_authorized(&code, || {
            polls.set(polls.get() + 1);
            async { Err(XblError::AuthorizationSlowDown) }
        })
        .await;
        assert!(matches!(result, Err(XblError::DeviceCodeExpired)));
        // Polls at 5 s and 15 s; the next (at 30 s) would pass 20 s.
        assert_eq!(polls.get(), 2);
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
