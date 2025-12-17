//! Authentication module for Xbox Live.
//!
//! This module provides the authentication flow for Xbox Live, specifically
//! designed for Minecraft Bedrock Edition integration.
//!
//! # Authentication Flow
//!
//! 1. **Device Code OAuth** - User authenticates via microsoft.com/link
//! 2. **Device Token** - Obtain device token with ECDSA-signed request
//! 3. **XBL Token** - Exchange for Xbox Live token via SISU endpoint
//!
//! # Client ID
//!
//! This library uses Minecraft's official client ID (`0000000048183522`).
//! This is **not** something you register in Azure AD — it's the hardcoded
//! client ID from the Minecraft client itself.
//!
//! **Why not register your own app?**
//! - Custom Azure AD apps appear as "YourApp" not "Minecraft" on Xbox Live
//! - They may not have access to Minecraft-specific relying parties
//! - Session broadcasting requires appearing as the official Minecraft client
//!
//! This approach is used by:
//! - [gophertunnel](https://github.com/Sandertv/gophertunnel)
//! - [MCXboxBroadcast](https://github.com/rtm516/MCXboxBroadcast)
//! - Other Minecraft Bedrock tools
//!
//! # Example
//!
//! ```no_run
//! use axolotl_xbl::auth::{DeviceCodeAuth, XblTokenClient, relying_party};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Step 1: Device code authentication
//!     let auth = DeviceCodeAuth::new();
//!     let code = auth.start().await?;
//!     
//!     println!("Go to {} and enter code: {}", code.verification_uri, code.user_code);
//!     
//!     let oauth_token = auth.wait_for_auth(&code).await?;
//!     
//!     // Step 2: Get XBL token
//!     let xbl_client = XblTokenClient::new();
//!     let xbl_token = xbl_client
//!         .get_xbl_token(&oauth_token, Some(relying_party::XBOX_LIVE))
//!         .await?;
//!     
//!     println!("Authenticated as: {} (XUID: {})", xbl_token.gamertag, xbl_token.xuid);
//!     println!("Auth header: {}", xbl_token.auth_header());
//!     
//!     Ok(())
//! }
//! ```

mod device_code;
mod signing;
mod xbl_token;

pub use device_code::{DeviceCodeAuth, DeviceCodeResponse, OAuthToken};
pub use signing::{SigningKeyPair, update_server_time_from_header};
pub use xbl_token::{XblToken, XblTokenClient, relying_party};

/// Minecraft's official client ID.
///
/// This is the client ID hardcoded in the Minecraft client.
/// Using this allows us to appear as "Minecraft" on Xbox Live,
/// which is required for session broadcasting and friends list features.
///
/// **Do not change this** unless you specifically want to authenticate
/// as a different application (which will break Minecraft integration).
pub const MINECRAFT_CLIENT_ID: &str = "0000000048183522";

/// Default scope for Xbox Live authentication.
///
/// This scope grants access to Xbox Live user authentication services.
pub const XBL_SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";

/// Device authentication endpoint.
pub const DEVICE_AUTH_URL: &str = "https://device.auth.xboxlive.com/device/authenticate";

/// SISU (Xbox Live) authorization endpoint.
pub const SISU_AUTHORIZE_URL: &str = "https://sisu.xboxlive.com/authorize";

/// Microsoft Live Connect device code endpoint.
pub const LIVE_CONNECT_URL: &str = "https://login.live.com/oauth20_connect.srf";

/// Microsoft Live Connect token endpoint.
pub const LIVE_TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
