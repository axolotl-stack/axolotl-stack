//! Error types for the Xbox Live API client.

use thiserror::Error;

/// Result type for Xbox Live API operations.
pub type XblResult<T> = Result<T, XblError>;

/// Errors that can occur during Xbox Live API operations.
#[derive(Error)]
pub enum XblError {
    /// HTTP request failed
    #[error("HTTP request failed")]
    Http(Option<reqwest::StatusCode>),

    /// JSON serialization/deserialization error
    #[error("Invalid JSON response")]
    Json(serde_json::error::Category),

    /// The remote service rejected an operation. Response bodies are never retained.
    #[error("{operation} failed: HTTP {status}")]
    HttpStatus {
        status: reqwest::StatusCode,
        operation: &'static str,
    },

    /// Numeric social-service error metadata; no remote strings or response bodies.
    #[error(
        "Xbox social operation failed: HTTP {status}, code {code:?}, retry after {retry_after:?}"
    )]
    SocialStatus {
        status: reqwest::StatusCode,
        code: Option<i64>,
        retry_after: Option<u64>,
    },

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    Auth(String),

    /// Device code expired before user authenticated
    #[error("Device code expired - user did not authenticate in time")]
    DeviceCodeExpired,

    /// OAuth device authorization asks the caller to increase its polling interval.
    #[error("Device authorization polling must slow down")]
    AuthorizationSlowDown,

    /// Xbox Live returned an error
    #[error("Xbox Live error: {0}")]
    XboxLive(String),

    /// Token is invalid or expired
    #[error("Token invalid or expired")]
    InvalidToken,

    /// API rate limited
    #[error("Rate limited - try again later")]
    RateLimited,

    /// Generic error
    #[error("{0}")]
    Other(String),
}

impl XblError {
    pub fn status(&self) -> Option<reqwest::StatusCode> {
        match self {
            Self::HttpStatus { status, .. } | Self::SocialStatus { status, .. } => Some(*status),
            Self::Http(status) => *status,
            _ => None,
        }
    }

    pub fn retry_after(&self) -> Option<u64> {
        match self {
            Self::SocialStatus { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    pub fn social_code(&self) -> Option<i64> {
        match self {
            Self::SocialStatus { code, .. } => *code,
            _ => None,
        }
    }

    /// Check if this error indicates an authentication/authorization failure.
    ///
    /// Returns true for 401 Unauthorized and 403 Forbidden errors,
    /// which typically indicate expired or invalid tokens.
    pub fn is_auth_error(&self) -> bool {
        self.status().is_some_and(|status| {
            status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN
        }) || matches!(self, Self::InvalidToken)
    }

    /// Parse Xbox Live error codes into human-readable messages.
    pub fn from_xbox_error_code(code: &str) -> Self {
        let message = match code {
            "2148916227" => "Account banned by Xbox for violating Community Standards",
            "2148916229" => "Account restricted - guardian has not given permission to play online",
            "2148916233" => "Account does not have an Xbox profile - create one at signup.live.com",
            "2148916234" => "Account has not accepted Xbox Terms of Service",
            "2148916235" => "Account is in a blocked region",
            "2148916236" => "Account requires proof of age",
            "2148916237" => "Account has reached playtime limit",
            "2148916238" => "Account is under 18 and must be added to a family",
            _ => return Self::XboxLive("Unknown Xbox error code".into()),
        };
        Self::XboxLive(message.to_string())
    }
}

// Error sources can disclose URLs/body fragments when anyhow prints a chain.
// Retain only typed, non-secret metadata, including through Error::source.
impl std::fmt::Debug for XblError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl From<reqwest::Error> for XblError {
    fn from(error: reqwest::Error) -> Self {
        Self::Http(error.status())
    }
}

impl From<serde_json::Error> for XblError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error.classify())
    }
}

pub(crate) fn check_status(response: &reqwest::Response, operation: &'static str) -> XblResult<()> {
    if let Some(date) = response
        .headers()
        .get("Date")
        .and_then(|value| value.to_str().ok())
    {
        crate::auth::update_server_time_from_header(date);
    }
    if !response.status().is_success() {
        return Err(XblError::HttpStatus {
            status: response.status(),
            operation,
        });
    }
    Ok(())
}

pub(crate) async fn response_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    operation: &'static str,
) -> XblResult<T> {
    check_status(&response, operation)?;
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).map_err(XblError::from)
}
