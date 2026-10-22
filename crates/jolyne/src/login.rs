//! The single seam between the protocol layer and `crate::auth`.
//!
//! Server phases carry an [`AuthenticatedLogin`]; everything that needs a
//! field of it goes through this module so the auth layer can evolve its
//! types without touching the state machines.

use crate::auth::ValidatedIdentity;
use crate::error::JolyneError;

/// A verified login: the player's identity (and, from the auth layer,
/// validated client data).
#[derive(Debug, Clone)]
pub struct AuthenticatedLogin {
    pub identity: ValidatedIdentity,
}

impl AuthenticatedLogin {
    /// Base64 SPKI key the encryption handshake derives the shared secret from.
    pub(crate) fn identity_public_key(&self) -> &str {
        &self.identity.identity_public_key
    }

    pub(crate) fn display_name(&self) -> &str {
        self.identity.display_name.as_deref().unwrap_or("")
    }
}

/// Verifies the `connection_request` of a `LoginPacket`.
pub(crate) async fn authenticate(
    connection_request: &[u8],
    online_mode: bool,
    allow_legacy: bool,
) -> Result<AuthenticatedLogin, JolyneError> {
    let (auth_info, client_data) = crate::auth::decode_login_request(connection_request)?;
    let identity =
        crate::auth::authenticate_login(&auth_info, &client_data, online_mode, allow_legacy)
            .await?;
    Ok(AuthenticatedLogin { identity })
}
