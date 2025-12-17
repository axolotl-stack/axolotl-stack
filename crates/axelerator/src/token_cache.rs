//! Token cache for Xbox Live authentication.
//!
//! Provides lazy-loading and automatic refresh of OAuth tokens with file-based persistence.
//!
//! # Example
//!
//! ```ignore
//! use axelerator::TokenCache;
//!
//! // Load or authenticate
//! let cache = TokenCache::from_file("token.json").await?;
//! let token = cache.get_or_authenticate().await?;
//!
//! // Token is now ready to use
//! println!("Authenticated as: {}", token.gamertag());
//! ```

use anyhow::{Context, Result};
use axolotl_xbl::auth::{DeviceCodeAuth, OAuthToken, XblToken, XblTokenClient, relying_party};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::OnceCell;
use tracing::{debug, info, warn};

/// Cached token data persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedToken {
    /// OAuth token from Microsoft.
    pub oauth: OAuthToken,
    /// Unix timestamp when token was acquired.
    pub acquired_at: u64,
    /// Unix timestamp when token expires.
    pub expires_at: u64,
}

impl CachedToken {
    /// Create a new cached token from an OAuth token.
    pub fn new(oauth: OAuthToken) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // OAuth tokens typically last 1 hour, refresh tokens last 90 days
        // We store the access token expiry for quick checks
        let expires_at = now + oauth.expires_in;

        Self {
            oauth,
            acquired_at: now,
            expires_at,
        }
    }

    /// Check if the access token has expired (or is about to).
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Consider expired if within 5 minutes of expiry
        now + 300 >= self.expires_at
    }

    /// Check if the refresh token is likely still valid.
    /// Refresh tokens last ~90 days.
    pub fn can_refresh(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Refresh tokens last about 90 days
        let refresh_expiry = self.acquired_at + (90 * 24 * 60 * 60);
        now < refresh_expiry
    }
}

/// Lazy-loading token cache with file persistence.
///
/// Handles:
/// - Loading tokens from disk
/// - Checking expiration
/// - Refreshing expired tokens
/// - Device code authentication for fresh tokens
/// - Saving refreshed tokens back to disk
pub struct TokenCache {
    /// Path to the cache file.
    path: PathBuf,
    /// Cached OAuth token (lazily loaded).
    oauth: OnceCell<OAuthToken>,
    /// Cached XBL token (lazily loaded).
    xbl: OnceCell<XblToken>,
    /// XBL token client - reused to share device token across all RP requests.
    xbl_client: XblTokenClient,
}

impl TokenCache {
    /// Create a new token cache backed by a file.
    ///
    /// Does not read the file immediately - use `get_or_authenticate()` to load.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            oauth: OnceCell::new(),
            xbl: OnceCell::new(),
            xbl_client: XblTokenClient::new(),
        }
    }

    /// Get or create an authenticated XBL token for the default relying party (Xbox Live).
    ///
    /// This is the main entry point. It will:
    /// 1. Try to load a cached token from disk
    /// 2. Refresh if expired but refresh token is valid
    /// 3. Do fresh device code auth if no valid token exists
    pub async fn get_or_authenticate(&self) -> Result<&XblToken> {
        self.xbl
            .get_or_try_init(|| async {
                let oauth = self.get_or_refresh_oauth().await?;

                info!("Exchanging OAuth token for XBL token...");
                let xbl_token = self
                    .xbl_client
                    .get_xbl_token(oauth, Some(relying_party::XBOX_LIVE))
                    .await
                    .context("Failed to get XBL token")?;

                info!(gamertag = %xbl_token.gamertag(), "Authenticated successfully");
                Ok(xbl_token)
            })
            .await
    }

    /// Get an XBL token for a specific relying party.
    ///
    /// This bypasses the memory cache (for now) to ensure we can get tokens for different RPs.
    pub async fn get_xbl_token(&self, rp: &str) -> Result<XblToken> {
        let oauth = self.get_or_refresh_oauth().await?;

        info!("Exchanging OAuth token for XBL token (RP: {})", rp);
        let xbl_token = self
            .xbl_client
            .get_xbl_token(oauth, Some(rp))
            .await
            .context("Failed to get XBL token")?;

        Ok(xbl_token)
    }

    /// Get or refresh the OAuth token.
    async fn get_or_refresh_oauth(&self) -> Result<&OAuthToken> {
        self.oauth
            .get_or_try_init(|| async {
                // Try loading from file
                if let Some(token) = self.try_load_from_file().await {
                    return Ok(token);
                }

                // No valid cached token, do fresh auth
                self.do_device_code_auth().await
            })
            .await
    }

    /// Try to load and validate a token from the cache file.
    async fn try_load_from_file(&self) -> Option<OAuthToken> {
        let data = tokio::fs::read_to_string(&self.path).await.ok()?;
        let cached: CachedToken = serde_json::from_str(&data).ok()?;

        debug!(path = %self.path.display(), "Found cached token");

        // If access token is still valid, use it directly
        if !cached.is_expired() {
            info!("Using cached token (still valid)");
            return Some(cached.oauth);
        }

        // If we can refresh, try that
        if cached.can_refresh() {
            info!("Token expired, attempting refresh...");
            match self.try_refresh(&cached.oauth).await {
                Ok(refreshed) => {
                    info!("Token refreshed successfully");
                    return Some(refreshed);
                }
                Err(e) => {
                    warn!("Token refresh failed: {}", e);
                    // Fall through to fresh auth
                }
            }
        } else {
            info!("Refresh token expired, need fresh authentication");
        }

        None
    }

    /// Attempt to refresh an expired token.
    async fn try_refresh(&self, old_token: &OAuthToken) -> Result<OAuthToken> {
        let auth = DeviceCodeAuth::new();
        let refreshed = auth
            .refresh(&old_token.refresh_token)
            .await
            .context("Refresh failed")?;

        // Save the refreshed token
        self.save_to_file(&refreshed).await?;

        Ok(refreshed)
    }

    /// Perform fresh device code authentication.
    async fn do_device_code_auth(&self) -> Result<OAuthToken> {
        let auth = DeviceCodeAuth::new();
        let code = auth
            .start()
            .await
            .context("Failed to start device code flow")?;

        // Print user-friendly login instructions
        println!();
        println!("╔══════════════════════════════════════════════════════════╗");
        println!("║                    XBOX LIVE LOGIN                       ║");
        println!("╠══════════════════════════════════════════════════════════╣");
        println!("║                                                          ║");
        println!("║  1. Open: {:<43} ║", code.verification_uri);
        println!("║  2. Enter code: {:<37} ║", code.user_code);
        println!("║                                                          ║");
        println!("╚══════════════════════════════════════════════════════════╝");
        println!();

        info!("Waiting for user to complete login...");

        let token = auth
            .wait_for_auth(&code)
            .await
            .context("Device code authentication failed or timed out")?;

        // Save for next time
        self.save_to_file(&token).await?;

        info!(path = %self.path.display(), "Token cached for future use");
        Ok(token)
    }

    /// Save a token to the cache file.
    async fn save_to_file(&self, token: &OAuthToken) -> Result<()> {
        let cached = CachedToken::new(token.clone());
        let json = serde_json::to_string_pretty(&cached).context("Failed to serialize token")?;

        tokio::fs::write(&self.path, json)
            .await
            .context("Failed to write token cache")?;

        debug!(path = %self.path.display(), "Token saved to cache");
        Ok(())
    }

    /// Get the path to the cache file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Clear the cached tokens (but not the file).
    /// Next call to `get_or_authenticate` will reload from file.
    pub fn clear(&mut self) {
        self.oauth = OnceCell::new();
        self.xbl = OnceCell::new();
    }

    /// Delete the cache file.
    pub async fn delete_file(&self) -> Result<()> {
        if self.path.exists() {
            tokio::fs::remove_file(&self.path)
                .await
                .context("Failed to delete token cache")?;
            info!(path = %self.path.display(), "Token cache deleted");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cached_token_expiry() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Token that expires in 1 hour
        let valid = CachedToken {
            oauth: OAuthToken {
                access_token: "test".into(),
                token_type: "bearer".into(),
                refresh_token: "test_refresh".into(),
                expires_in: 3600,
                user_id: None,
            },
            acquired_at: now,
            expires_at: now + 3600,
        };
        assert!(!valid.is_expired());
        assert!(valid.can_refresh());

        // Token that expired 1 hour ago
        let expired = CachedToken {
            oauth: OAuthToken {
                access_token: "test".into(),
                token_type: "bearer".into(),
                refresh_token: "test_refresh".into(),
                expires_in: 3600,
                user_id: None,
            },
            acquired_at: now - 7200,
            expires_at: now - 3600,
        };
        assert!(expired.is_expired());
        assert!(expired.can_refresh()); // Refresh token still valid
    }
}
