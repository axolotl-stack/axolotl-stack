//! Error types for the Xbox Live API client.

use thiserror::Error;

/// Result type for Xbox Live API operations.
pub type XblResult<T> = Result<T, XblError>;

/// Errors that can occur during Xbox Live API operations.
#[derive(Debug, Error)]
pub enum XblError {
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    Auth(String),

    /// Device code expired before user authenticated
    #[error("Device code expired - user did not authenticate in time")]
    DeviceCodeExpired,

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
            _ => return Self::XboxLive(format!("Unknown error code: {}", code)),
        };
        Self::XboxLive(message.to_string())
    }
}
