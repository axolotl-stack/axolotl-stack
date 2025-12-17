//! Xbox Live API client for the axolotl-stack.
//!
//! This crate provides authentication and API access to Xbox Live services
//! for Minecraft Bedrock Edition, including:
//!
//! - **Auth**: Microsoft device code authentication and Xbox Live tokens
//! - **Friends**: Friend list management and social APIs
//! - **Sessions**: Xbox session creation for multiplayer visibility
//! - **Presence**: Online status updates
//! - **PlayFab**: Minecraft session start for signaling
//!
//! # Quick Start
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
//!     // Step 2: Get Xbox Live token
//!     let xbl_client = XblTokenClient::new();
//!     let xbl_token = xbl_client
//!         .get_xbl_token(&oauth_token, Some(relying_party::XBOX_LIVE))
//!         .await?;
//!     
//!     println!("Authenticated as: {}", xbl_token.gamertag());
//!     
//!     Ok(())
//! }
//! ```

pub mod auth;
pub mod constants;
pub mod error;
pub mod friends;
pub mod playfab;
pub mod presence;
pub mod rta;
pub mod session;

// Re-exports for convenience
pub use auth::{DeviceCodeAuth, OAuthToken, XblToken, XblTokenClient};
pub use constants::{SERVICE_CONFIG_ID, TEMPLATE_NAME, TITLE_ID, endpoints};
pub use error::{XblError, XblResult};
pub use friends::{FriendsClient, Person, SocialSummary};
pub use playfab::PlayFabClient;
pub use presence::{PresenceClient, PresenceState};
pub use rta::RtaClient;
pub use session::{ExpandedSessionInfo, SessionClient, SessionInfo};
