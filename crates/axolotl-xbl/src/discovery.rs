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
//!     let services = client.discover("1.26.51").await?;
//!
//!     println!("Signaling: {}", services.signaling.service_uri);
//!     println!("STUN: {:?}", services.signaling.stun_uri);
//!     println!("TURN: {:?}", services.signaling.turn_uri);
//!
//!     Ok(())
//! }
//! ```

use crate::error::{XblError, check_status, response_json};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, info};

static DISCOVERY_CACHE: LazyLock<Mutex<HashMap<String, DiscoveryResponse>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// gophertunnel `minecraft/service/discovery.go:discoveryURL/Discover`.
pub const DISCOVERY_URL: &str =
    "https://client.discovery.minecraft-services.net/api/v1.0/discovery";

/// Default game identifier for Bedrock Edition.
pub const GAME_ID: &str = "MinecraftPE";

/// Discovery API errors.
#[derive(Error)]
pub enum DiscoveryError {
    #[error("HTTP request failed")]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Service(#[from] XblError),

    #[error("Missing required service: {0}")]
    MissingService(String),

    #[error("Version not supported: {0}")]
    UnsupportedVersion(String),
}

impl std::fmt::Debug for DiscoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
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
    /// - `version`: The caller's supported game version (e.g., "1.26.51").
    ///
    /// # Returns
    /// Parsed service endpoints including signaling, auth, and other services.
    pub async fn discover(&self, version: &str) -> DiscoveryResult<ServiceEndpoints> {
        self.discover_at(DISCOVERY_URL, version).await
    }

    /// Resolve only the auth environment, without requiring unrelated services.
    /// gophertunnel `discovery.go:Discovery.Environment` and
    /// `token.go:AuthorizationEnvironment.UnmarshalJSON`.
    pub async fn discover_authorization(&self, version: &str) -> DiscoveryResult<AuthEndpoint> {
        Self::authorization(&self.discover_raw(version).await?)
    }

    fn authorization(response: &DiscoveryResponse) -> DiscoveryResult<AuthEndpoint> {
        let auth: ServiceConfig = response.environment("auth")?;
        let service_uri = auth
            .service_uri
            .as_ref()
            .filter(|uri| !uri.is_empty())
            .ok_or_else(|| DiscoveryError::MissingService("auth.serviceUri".into()))?;
        let issuer = auth
            .issuer
            .as_ref()
            .filter(|uri| !uri.is_empty())
            .ok_or_else(|| DiscoveryError::MissingService("auth.issuer".into()))?;
        for uri in [service_uri, issuer] {
            reqwest::Url::parse(uri).map_err(|_| {
                XblError::Auth("Invalid Minecraft authorization environment".into())
            })?;
        }
        Ok(AuthEndpoint {
            service_uri: service_uri.clone(),
            issuer: Some(issuer.clone()),
            playfab_title_id: auth.playfab_title_id.clone(),
        })
    }

    /// gophertunnel `discovery.go:Discover`: process-wide URL-keyed cache.
    pub async fn discover_raw(&self, version: &str) -> DiscoveryResult<DiscoveryResponse> {
        self.discover_raw_at(DISCOVERY_URL, version).await
    }
    async fn discover_raw_at(
        &self,
        endpoint: &str,
        version: &str,
    ) -> DiscoveryResult<DiscoveryResponse> {
        let mut url = reqwest::Url::parse(endpoint)
            .map_err(|_| XblError::Auth("Invalid discovery endpoint".into()))?;
        url.path_segments_mut()
            .map_err(|_| XblError::Auth("Invalid discovery endpoint".into()))?
            .extend([self.game_id.as_str(), "builds", version]);

        let mut cache = DISCOVERY_CACHE.lock().await;
        if let Some(response) = cache.get(url.as_str()) {
            return Ok(response.clone());
        }
        debug!(target: "discovery", "Fetching service discovery");
        let response = self
            .client
            .get(url.clone())
            .header("Content-Type", "application/json")
            .header("User-Agent", crate::playfab::SERVICE_USER_AGENT)
            .send()
            .await?;
        check_status(&response, "Minecraft service discovery")?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(XblError::HttpStatus {
                status: response.status(),
                operation: "Minecraft service discovery",
            }
            .into());
        }
        let response: DiscoveryResponse =
            response_json(response, "Minecraft service discovery").await?;
        cache.insert(url.into(), response.clone());
        Ok(response)
    }

    async fn discover_at(
        &self,
        endpoint: &str,
        version: &str,
    ) -> DiscoveryResult<ServiceEndpoints> {
        let response = self.discover_raw_at(endpoint, version).await?;

        let signaling: ServiceConfig = response.environment("signaling")?;

        let auth = Self::authorization(&response)?;

        let persona: Option<ServiceConfig> = if response
            .result
            .service_environments
            .get("persona")
            .is_some_and(|environments| environments.contains_key("prod"))
        {
            Some(response.environment("persona")?)
        } else {
            None
        };

        let endpoints = ServiceEndpoints {
            signaling: SignalingEndpoint {
                service_uri: signaling.service_uri.clone().ok_or_else(|| {
                    DiscoveryError::MissingService("signaling.serviceUri".to_string())
                })?,
                stun_uri: signaling.stun_uri.clone(),
                turn_uri: signaling.turn_uri.clone(),
            },
            auth,
            persona: persona.and_then(|p| {
                Some(PersonaEndpoint {
                    service_uri: p.service_uri.clone()?,
                    playfab_title_id: p.playfab_title_id.clone(),
                })
            }),
            raw: response.result.service_environments,
        };

        info!(target: "discovery", "Discovery complete");

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
#[derive(Debug, Clone, Deserialize)]
pub struct DiscoveryResponse {
    pub result: DiscoveryResult_,
}

impl DiscoveryResponse {
    /// Decode only the requested production environment, as Go
    /// `minecraft/service/discovery.go:Discovery.Environment` does.
    pub fn environment<T: serde::de::DeserializeOwned>(&self, name: &str) -> DiscoveryResult<T> {
        let value = self
            .result
            .service_environments
            .get(name)
            .and_then(|environments| environments.get("prod"))
            .ok_or_else(|| DiscoveryError::MissingService(name.into()))?;
        T::deserialize(value).map_err(|error| DiscoveryError::Service(XblError::from(error)))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResult_ {
    pub service_environments: HashMap<String, HashMap<String, serde_json::Value>>,
    #[serde(default)]
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
    #[serde(default, rename = "playFabTitleId", alias = "playfabTitleId")]
    pub playfab_title_id: Option<String>,
    #[serde(default)]
    pub edu_play_fab_title_id: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
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
    pub raw: HashMap<String, HashMap<String, serde_json::Value>>,
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
    async fn discovery_checks_status_and_redacts_malformed_responses() {
        for (status, body) in [
            (403, "SECRET denied"),
            (200, r#"{"result":{"serviceEnvironments":"SECRET"}}"#),
        ] {
            let (url, task) = crate::playfab::tests::server(status, body).await;
            let error = DiscoveryClient::new()
                .discover_at(&url, "1.26.51")
                .await
                .unwrap_err();
            crate::playfab::tests::assert_safe(&error, &error);
            if status == 403 {
                assert!(
                    matches!(error, DiscoveryError::Service(crate::error::XblError::HttpStatus { status, .. }) if status == reqwest::StatusCode::FORBIDDEN)
                );
            } else {
                assert!(matches!(
                    error,
                    DiscoveryError::Service(crate::error::XblError::Json(_))
                ));
            }
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn discovery_transmits_version_and_selects_production_services() {
        let (url, task) = crate::playfab::tests::server(200, r#"{"result":{"serviceEnvironments":{"signaling":{"prod":{"serviceUri":"wss://prod.example","stunUri":"stun:example","turnUri":"turn:example"},"test":{"serviceUri":"wss://test.example"}},"auth":{"prod":{"serviceUri":"https://auth.example","issuer":"https://issuer.example","playFabTitleId":"20CA2"}}},"supportedEnvironments":{"1.26.51":["prod"]}}}"#).await;
        let endpoints = DiscoveryClient::new()
            .discover_at(&url, "1.26.51")
            .await
            .unwrap();
        assert_eq!(endpoints.signaling.service_uri, "wss://prod.example");
        assert_eq!(endpoints.auth.service_uri, "https://auth.example");
        let request = task.await.unwrap();
        assert!(request.starts_with("GET /MinecraftPE/builds/1.26.51 HTTP/1.1\r\n"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("user-agent: libhttpclient/1.0.0.0")
        );
        assert_eq!(endpoints.auth.playfab_title_id.as_deref(), Some("20CA2"));
        // Go's process-wide cache survives client recreation; the server is closed.
        assert_eq!(
            DiscoveryClient::new()
                .discover_at(&url, "1.26.51")
                .await
                .unwrap()
                .auth
                .service_uri,
            "https://auth.example"
        );
    }

    #[tokio::test]
    async fn authorization_does_not_require_signaling_or_supported_environments() {
        let (url, task) = crate::playfab::tests::server(200, r#"{"result":{"serviceEnvironments":{"auth":{"prod":{"serviceUri":"https://auth.example","issuer":"https://issuer.example","playFabTitleId":"20CA2"}}}}}"#).await;
        let raw = DiscoveryClient::new()
            .discover_raw_at(&url, "1.26.51")
            .await
            .unwrap();
        assert_eq!(
            DiscoveryClient::authorization(&raw)
                .unwrap()
                .issuer
                .as_deref(),
            Some("https://issuer.example")
        );
        task.await.unwrap();
    }
}
