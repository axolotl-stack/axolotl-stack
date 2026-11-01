//! Authenticated HTTP-signaled NetherNet client. Uses the ordinary PlayFab/Xbox
//! flow and the same per-connection key/token for SDP and Bedrock Login.
//!
//! ```text
//! cargo run -p jolyne --example nethernet_endpoint_client --features endpoint -- \
//!   https://server.example.org:443 --token-cache /path/to/oauth-cache.json \
//!   --ice-server stun:stun.example.org:3478
//! ```
//! Without `--token-cache`, uses the user's cache directory. A missing cache
//! starts the normal device-code sign-in. Flat OAuth caches with absolute RFC3339
//! expiry are imported read-only and must be refreshed by their original owner.
//! Tokens, keys and payloads are not printed.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, ensure};
use axelerator::TokenCache;
use axolotl_xbl::PlayFabClient;
use axolotl_xbl::auth::{OAuthToken, XblToken, XblTokenClient, relying_party};
use axolotl_xbl::discovery::DiscoveryClient;
use clap::Parser;
use futures::future::BoxFuture;
use jolyne::error::AuthError;
use jolyne::stream::client::MultiplayerTokenSource;
use jolyne::stream::{Handshake, NetherNetTransport};
use jolyne::{BedrockStream, Client, ClientConfig, ClientHandshakeConfig, GAME_VERSION};
use p384::SecretKey;
use p384::pkcs8::EncodePublicKey;
use tokio_nethernet::identity::Identity;
use tokio_nethernet::{IceServer, NetherNetDialer, NetherNetDialerConfig};
use uuid::Uuid;

/// Join an HTTP endpoint with an Xbox account and observe spawned play traffic.
#[derive(Parser)]
struct Arguments {
    /// HTTP(S) endpoint URL with an explicit port and no path.
    endpoint: String,
    /// OAuth token cache; defaults to a file in the user's cache directory.
    #[arg(long)]
    token_cache: Option<PathBuf>,
    /// Explicit STUN/TURN service URL without embedded credentials; repeatable.
    #[arg(long)]
    ice_server: Vec<String>,
    /// Maximum time for authentication, connection establishment and world join.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=300))]
    timeout_seconds: u64,
    /// Remain in the joined world for this many seconds, without sending chat.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=300))]
    observe_seconds: u64,
}

/// One service-issued token is valid for exactly one connection public key.
/// Reusing it for SDP and Login avoids separate issuance or key mismatches.
struct BoundToken {
    key: Vec<u8>,
    token: String,
}
impl MultiplayerTokenSource for BoundToken {
    fn multiplayer_token<'a>(
        &'a self,
        public_key_der: &'a [u8],
    ) -> BoxFuture<'a, std::result::Result<String, AuthError>> {
        Box::pin(async move {
            if self.key != public_key_der {
                return Err(AuthError::TokenAcquisition(
                    "Token belongs to a different connection key".into(),
                ));
            }
            Ok(self.token.clone())
        })
    }
}

/// Use the user's OS cache location, never an implicit repository credential file.
fn default_cache_path() -> Result<PathBuf> {
    let root = std::env::var_os("XDG_CACHE_HOME")
        .or_else(|| std::env::var_os("LOCALAPPDATA"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| anyhow!("No user cache directory is configured; pass --token-cache"))?;
    Ok(root.join("jolyne").join("oauth.json"))
}

/// A flat cache belongs to its original OAuth application. Import only its
/// unexpired access token; never guess its client ID or refresh/rewrite it.
struct ImportedOAuth {
    oauth: OAuthToken,
    expiry: i64,
}
impl ImportedOAuth {
    /// Parse absolute expiry before making any network request. Parse errors
    /// never include the credential-bearing JSON or values.
    fn parse(value: &serde_json::Value, now: i64) -> Result<Self> {
        let access_token = value
            .get("access_token")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("Imported OAuth cache lacks an access token"))?;
        let token_type = value
            .get("token_type")
            .and_then(|value| value.as_str())
            .filter(|value| value.eq_ignore_ascii_case("Bearer"))
            .ok_or_else(|| anyhow!("Imported OAuth cache has an unsupported token type"))?;
        let expiry = value
            .get("expiry")
            .and_then(|value| value.as_str())
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.timestamp())
            .ok_or_else(|| anyhow!("Imported OAuth cache requires an absolute RFC3339 expiry"))?;
        ensure!(
            expiry > now.saturating_add(60),
            "Imported OAuth cache is expired or near expiry; refresh it with its original sign-in application"
        );
        Ok(Self {
            oauth: OAuthToken {
                access_token: access_token.into(),
                token_type: token_type.into(),
                refresh_token: String::new(),
                expires_in: (expiry - now) as u64,
                user_id: None,
            },
            expiry,
        })
    }
    /// Every service exchange rechecks the imported access-token deadline.
    fn current(&self) -> Result<&OAuthToken> {
        ensure!(
            self.expiry > chrono::Utc::now().timestamp().saturating_add(10),
            "Imported OAuth cache expired; refresh it with its original sign-in application"
        );
        Ok(&self.oauth)
    }
}

/// Native device-code caches refresh through their known application. Flat
/// imported caches are read-only and retain no usable refresh token in memory.
enum Authorization {
    Native(Box<TokenCache>),
    Imported {
        token: ImportedOAuth,
        client: Box<XblTokenClient>,
    },
}
impl Authorization {
    /// Classify the cache without overwriting malformed or foreign credentials.
    async fn load(path: PathBuf) -> Result<Self> {
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::Native(Box::new(TokenCache::new(path))));
            }
            Err(_) => return Err(anyhow!("Cannot read OAuth cache")),
        };
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| anyhow!("Cannot parse OAuth cache"))?;
        if value.get("oauth").is_some() {
            return Ok(Self::Native(Box::new(TokenCache::new(path))));
        }
        Ok(Self::Imported {
            token: ImportedOAuth::parse(&value, chrono::Utc::now().timestamp())?,
            client: Box::new(XblTokenClient::new()),
        })
    }
    /// Exchange the OAuth access token through the standard Xbox relying party.
    async fn account(&self) -> Result<XblToken> {
        match self {
            Self::Native(cache) => cache
                .get_or_authenticate()
                .await
                .map_err(|_| anyhow!("Xbox sign-in failed")),
            Self::Imported { token, client } => client
                .get_xbl_token(token.current()?, Some(relying_party::XBOX_LIVE))
                .await
                .map_err(|_| anyhow!("Imported OAuth Xbox exchange failed")),
        }
    }
    /// Resolve the PlayFab relying party with the normal NSAL token client.
    async fn for_url(&self, url: &str) -> Result<XblToken> {
        match self {
            Self::Native(cache) => cache
                .get_xbl_token_for_url(url)
                .await
                .map_err(|_| anyhow!("PlayFab-scoped Xbox authorization failed")),
            Self::Imported { token, client } => client
                .get_xbl_token_for_url(token.current()?, url)
                .await
                .map_err(|_| anyhow!("Imported OAuth PlayFab authorization failed")),
        }
    }
}

/// Acquire the normal service-issued multiplayer JWT for one P-384 key.
/// Authentication errors are intentionally summarized without token responses.
async fn authenticated_identity(
    cache_path: PathBuf,
) -> Result<(ClientHandshakeConfig, Arc<Identity>)> {
    if let Some(parent) = cache_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent)
            .await
            .context("Cannot create token cache directory")?;
    }
    let authorization = Authorization::load(cache_path).await?;
    let account = authorization.account().await?;
    let environment = DiscoveryClient::new()
        .discover_authorization(GAME_VERSION)
        .await
        .map_err(|_| anyhow!("Minecraft authorization discovery failed"))?;
    let domain = reqwest::Url::parse(
        environment
            .issuer
            .as_deref()
            .unwrap_or(&environment.service_uri),
    )?
    .host_str()
    .ok_or_else(|| anyhow!("Authorization issuer has no host"))?
    .to_owned();
    let playfab = PlayFabClient::with_environment(&environment)
        .map_err(|_| anyhow!("Invalid Minecraft authorization environment"))?;
    let scoped = authorization.for_url(playfab.login_url()).await?;
    let ticket = playfab
        .login(&scoped.user_hash, &scoped.token)
        .await
        .map_err(|_| anyhow!("PlayFab sign-in failed"))?;
    let service = playfab
        .service_token(&Uuid::new_v4().to_string(), &ticket, GAME_VERSION)
        .await
        .map_err(|_| anyhow!("Minecraft service authorization failed"))?;
    let key = SecretKey::random(&mut rand::thread_rng());
    let public = key.public_key().to_public_key_der()?.as_bytes().to_vec();
    let token = playfab
        .multiplayer_token(&service, &public)
        .await
        .map_err(|_| anyhow!("Per-key multiplayer authorization failed"))?;
    let source = Arc::new(BoundToken {
        key: public,
        token: token.clone(),
    });
    let identity = Arc::new(Identity {
        private_key: p384::ecdsa::SigningKey::from(key.clone()),
        token,
        domain,
    });
    let login = ClientHandshakeConfig::with_xbox_live(
        SocketAddr::from(([0, 0, 0, 0], 0)),
        key,
        account.gamertag(),
        Uuid::new_v4(),
        source,
    );
    Ok((login, identity))
}

/// Reach HTTP status, authenticate, complete DTLS and enter the actual world.
async fn run(arguments: Arguments) -> Result<()> {
    ensure!(
        arguments.ice_server.len() <= 16,
        "At most sixteen ICE servers are supported"
    );
    for server in &arguments.ice_server {
        ensure!(
            server.len() <= 2048
                && (server.starts_with("stun:") || server.starts_with("turn:"))
                && !server.contains('@'),
            "ICE server must be a bounded stun: or turn: URL without embedded credentials"
        );
    }
    let signaling = tokio_nethernet::endpoint::Client::new();
    let status = signaling
        .status(&arguments.endpoint)
        .await
        .context("HTTP endpoint status failed")?;
    println!(
        "HTTP status reachable: protocol {}, players {}/{}",
        status.protocol, status.player_count, status.max_player_count
    );
    let cache = arguments.token_cache.map_or_else(default_cache_path, Ok)?;
    let (login, identity) = authenticated_identity(cache).await?;
    println!("Account authorization ready; connecting with a bound identity key");
    let dialer = NetherNetDialer::connect_with_signaling(
        signaling,
        NetherNetDialerConfig {
            identity: Some(identity),
            ice_servers: arguments
                .ice_server
                .into_iter()
                .map(|url| IceServer {
                    urls: vec![url],
                    username: String::new(),
                    password: String::new(),
                })
                .collect(),
            ..Default::default()
        },
    );
    let transport = dialer
        .dial(arguments.endpoint)
        .await
        .context("WebRTC connection failed")?;
    println!("WebRTC/DTLS transport established");
    let handshake = BedrockStream::<Handshake, Client, _>::from_transport(
        NetherNetTransport::new(transport),
        ClientConfig {
            join_timeout: Duration::from_secs(arguments.timeout_seconds),
            ..Default::default()
        },
    )
    .await
    .context("Bedrock settings negotiation failed")?;
    let (mut play, data) = handshake
        .join(login)
        .await
        .context("Authenticated world join failed")?;
    println!(
        "Joined world: dimension {}, runtime ID {}",
        data.start_game.settings.spawn_settings.dimension,
        play.runtime_id()
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(arguments.observe_seconds);
    let mut packets = 0u64;
    loop {
        match tokio::time::timeout_at(deadline, play.recv_packet()).await {
            Ok(Ok(_)) => packets += 1,
            Ok(Err(error)) if error.is_recoverable() => {}
            Ok(Err(_)) => return Err(anyhow!("Server disconnected during world observation")),
            Err(_) => break,
        }
    }
    println!("Observed {packets} play packets after authenticated spawn");
    play.disconnect(
        jolyne::valentine::EnumsConnectionDisconnectFailReason::Disconnected,
        "",
    )
    .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let timeout = Duration::from_secs(
        arguments
            .timeout_seconds
            .saturating_add(arguments.observe_seconds),
    );
    tokio::time::timeout(timeout, run(arguments))
        .await
        .context("Endpoint client deadline exceeded")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn token_source_refuses_another_connection_key() {
        let source = BoundToken {
            key: vec![1, 2, 3],
            token: "test-only-token".into(),
        };
        assert!(source.multiplayer_token(&[1, 2, 4]).await.is_err());
        assert_eq!(
            source.multiplayer_token(&[1, 2, 3]).await.unwrap(),
            "test-only-token"
        );
    }

    #[test]
    fn command_line_has_no_offline_or_identity_bypass_and_bounds_time() {
        let args = Arguments::try_parse_from([
            "client",
            "https://server.example.org:443",
            "--token-cache",
            "/tmp/user-cache.json",
            "--ice-server",
            "stun:stun.example.org:3478",
        ])
        .unwrap();
        assert!(args.token_cache.is_some());
        assert_eq!(args.ice_server.len(), 1);
        assert!(
            Arguments::try_parse_from(["client", "https://server.example.org:443", "--offline"])
                .is_err()
        );
        assert!(
            Arguments::try_parse_from([
                "client",
                "https://server.example.org:443",
                "--timeout-seconds",
                "0"
            ])
            .is_err()
        );
    }

    #[test]
    fn imported_flat_cache_uses_absolute_expiry_and_discards_refresh_credentials() {
        let value = serde_json::json!({
            "access_token":"example-access", "token_type":"Bearer", "refresh_token":"example-refresh",
            "expiry":"2023-11-14T22:15:00Z", "expires_in":999999, "extra_sign_in_marker":"ignored"
        });
        let imported = ImportedOAuth::parse(&value, 1_700_000_000).unwrap();
        assert_eq!(imported.oauth.expires_in, 100);
        assert!(imported.oauth.refresh_token.is_empty());
        assert!(ImportedOAuth::parse(&value, 1_700_000_040).is_err());
        assert!(ImportedOAuth::parse(&value, 1_700_000_100).is_err());
        let mut malformed = value.clone();
        malformed["expiry"] = "credential-bearing-invalid-value".into();
        let error = ImportedOAuth::parse(&malformed, 1_700_000_000)
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains("credential-bearing"));
        assert!(!error.contains("example-access"));
    }

    #[tokio::test]
    async fn expired_import_is_rejected_without_rewriting_or_starting_another_auth_flow() {
        let path =
            std::env::temp_dir().join(format!("jolyne-imported-oauth-{}.json", Uuid::new_v4()));
        let bytes = br#"{"access_token":"example-access","token_type":"Bearer","refresh_token":"example-refresh","expiry":"2000-01-01T00:00:00Z","expires_in":999999}"#;
        tokio::fs::write(&path, bytes).await.unwrap();
        let result = Authorization::load(path.clone()).await;
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("original sign-in application")
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), bytes);
        tokio::fs::remove_file(path).await.unwrap();
    }
}
