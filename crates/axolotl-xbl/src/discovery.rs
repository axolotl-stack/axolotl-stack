//! Minecraft services discovery client.
//!
//! This module fetches service endpoints from Minecraft's discovery API,
//! including regional signaling WebSocket URLs and TURN/STUN servers.
//!
//! # Overview
//!
//! The discovery API provides environment-specific service URIs based on
//! the client's game version. Most importantly for NetherNet:
//!
//! - **Signaling**: Regional WebSocket URL (e.g., `wss://signal-eastus2.franchise.minecraft-services.net`)
//! - **STUN/TURN**: ICE server URIs for WebRTC connectivity
//!
//! # Usage
//!
//! ```no_run
//! use axolotl_xbl::discovery::DiscoveryClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = DiscoveryClient::new();
//!     let services = client.discover("1.21.131").await?;
//!
//!     println!("Signaling: {}", services.signaling.service_uri);
//!     println!("STUN: {:?}", services.signaling.stun_uri);
//!     println!("TURN: {:?}", services.signaling.turn_uri);
//!
//!     Ok(())
//! }
//! ```

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, info};

/// Discovery API base URL.
pub const DISCOVERY_URL: &str =
    "https://client.discovery.minecraft-services.net/api/v1.0/discovery";

/// Default game identifier for Bedrock Edition.
pub const GAME_ID: &str = "MinecraftPE";

/// Discovery API errors.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Failed to parse discovery response: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("Missing required service: {0}")]
    MissingService(String),

    #[error("Version not supported: {0}")]
    UnsupportedVersion(String),
}

/// Result type for discovery operations.
pub type DiscoveryResult<T> = Result<T, DiscoveryError>;

/// Discovery API client.
#[derive(Debug, Clone)]
pub struct DiscoveryClient {
    client: Client,
    game_id: String,
}

impl Default for DiscoveryClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscoveryClient {
    /// Create a new discovery client with default settings.
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            game_id: GAME_ID.to_string(),
        }
    }

    /// Create a discovery client with a custom HTTP client.
    pub fn with_client(client: Client) -> Self {
        Self {
            client,
            game_id: GAME_ID.to_string(),
        }
    }

    /// Discover service endpoints for a specific game version.
    ///
    /// # Arguments
    /// - `version`: Game version string (e.g., "1.21.131")
    ///
    /// # Returns
    /// Parsed service endpoints including signaling, auth, and other services.
    pub async fn discover(&self, version: &str) -> DiscoveryResult<ServiceEndpoints> {
        let url = format!("{}/{}/builds/{}", DISCOVERY_URL, self.game_id, version);

        debug!(target: "discovery", url = %url, "Fetching service discovery");

        let response: DiscoveryResponse = self.client.get(&url).send().await?.json().await?;

        // Extract the prod environment (most common)
        let env = "prod";

        let signaling = response
            .result
            .service_environments
            .get("signaling")
            .and_then(|s| s.get(env))
            .ok_or_else(|| DiscoveryError::MissingService("signaling".to_string()))?;

        let auth = response
            .result
            .service_environments
            .get("auth")
            .and_then(|s| s.get(env))
            .ok_or_else(|| DiscoveryError::MissingService("auth".to_string()))?;

        let persona = response
            .result
            .service_environments
            .get("persona")
            .and_then(|s| s.get(env));

        let endpoints = ServiceEndpoints {
            signaling: SignalingEndpoint {
                service_uri: signaling.service_uri.clone().ok_or_else(|| {
                    DiscoveryError::MissingService("signaling.serviceUri".to_string())
                })?,
                stun_uri: signaling.stun_uri.clone(),
                turn_uri: signaling.turn_uri.clone(),
            },
            auth: AuthEndpoint {
                service_uri: auth
                    .service_uri
                    .clone()
                    .ok_or_else(|| DiscoveryError::MissingService("auth.serviceUri".to_string()))?,
                issuer: auth.issuer.clone(),
                playfab_title_id: auth.playfab_title_id.clone(),
            },
            persona: persona.and_then(|p| {
                Some(PersonaEndpoint {
                    service_uri: p.service_uri.clone()?,
                    playfab_title_id: p.playfab_title_id.clone(),
                })
            }),
            raw: response.result.service_environments,
        };

        info!(
            target: "discovery",
            signaling_uri = %endpoints.signaling.service_uri,
            stun = ?endpoints.signaling.stun_uri,
            turn = ?endpoints.signaling.turn_uri,
            "Discovery complete"
        );

        Ok(endpoints)
    }

    /// Discover and return just the signaling WebSocket URL.
    ///
    /// This is a convenience method for when you only need the signaling endpoint.
    pub async fn discover_signaling_url(&self, version: &str) -> DiscoveryResult<String> {
        let endpoints = self.discover(version).await?;
        Ok(endpoints.signaling.service_uri)
    }
}

/// Raw discovery API response.
#[derive(Debug, Deserialize)]
pub struct DiscoveryResponse {
    pub result: DiscoveryResult_,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResult_ {
    pub service_environments: HashMap<String, HashMap<String, ServiceConfig>>,
    pub supported_environments: HashMap<String, Vec<String>>,
}

/// Configuration for a single service from discovery.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceConfig {
    #[serde(default)]
    pub service_uri: Option<String>,
    #[serde(default)]
    pub stun_uri: Option<String>,
    #[serde(default)]
    pub turn_uri: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub playfab_title_id: Option<String>,
    #[serde(default)]
    pub edu_play_fab_title_id: Option<String>,
}

/// Parsed service endpoints for easy access.
#[derive(Debug, Clone)]
pub struct ServiceEndpoints {
    /// Signaling service (WebSocket, STUN, TURN).
    pub signaling: SignalingEndpoint,
    /// Auth service.
    pub auth: AuthEndpoint,
    /// Persona service (optional).
    pub persona: Option<PersonaEndpoint>,
    /// Raw service configurations for additional services.
    pub raw: HashMap<String, HashMap<String, ServiceConfig>>,
}

/// Signaling service endpoint.
#[derive(Debug, Clone)]
pub struct SignalingEndpoint {
    /// Regional WebSocket URL (e.g., `wss://signal-eastus2.franchise.minecraft-services.net`).
    pub service_uri: String,
    /// STUN server URI (e.g., `stun:turn.azure.com:3478`).
    pub stun_uri: Option<String>,
    /// TURN server URI (e.g., `turn:turn.azure.com:3478`).
    pub turn_uri: Option<String>,
}

impl SignalingEndpoint {
    /// Build the full WebSocket URL for a nethernet ID.
    ///
    /// # Example
    /// ```
    /// use axolotl_xbl::discovery::SignalingEndpoint;
    ///
    /// let endpoint = SignalingEndpoint {
    ///     service_uri: "wss://signal-eastus2.franchise.minecraft-services.net".to_string(),
    ///     stun_uri: None,
    ///     turn_uri: None,
    /// };
    ///
    /// assert_eq!(
    ///     endpoint.websocket_url(12345),
    ///     "wss://signal-eastus2.franchise.minecraft-services.net/ws/v1.0/signaling/12345"
    /// );
    /// ```
    pub fn websocket_url(&self, nethernet_id: u64) -> String {
        format!("{}/ws/v1.0/signaling/{}", self.service_uri, nethernet_id)
    }
}

/// Auth service endpoint.
#[derive(Debug, Clone)]
pub struct AuthEndpoint {
    /// Service URI for session start.
    pub service_uri: String,
    /// Token issuer.
    pub issuer: Option<String>,
    /// PlayFab title ID.
    pub playfab_title_id: Option<String>,
}

/// Persona service endpoint.
#[derive(Debug, Clone)]
pub struct PersonaEndpoint {
    /// Service URI.
    pub service_uri: String,
    /// PlayFab title ID.
    pub playfab_title_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_url_building() {
        let endpoint = SignalingEndpoint {
            service_uri: "wss://signal-eastus2.franchise.minecraft-services.net".to_string(),
            stun_uri: Some("stun:turn.azure.com:3478".to_string()),
            turn_uri: Some("turn:turn.azure.com:3478".to_string()),
        };

        assert_eq!(
            endpoint.websocket_url(12345678901234),
            "wss://signal-eastus2.franchise.minecraft-services.net/ws/v1.0/signaling/12345678901234"
        );
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_discovery_live() {
        let client = DiscoveryClient::new();
        let endpoints = client.discover("1.21.131").await.unwrap();

        assert!(endpoints.signaling.service_uri.starts_with("wss://"));
        assert!(endpoints.signaling.stun_uri.is_some());
        assert!(endpoints.signaling.turn_uri.is_some());
    }
}
