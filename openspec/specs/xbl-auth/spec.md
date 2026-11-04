# xbl-auth Specification

## Purpose
Xbox Live and Minecraft authentication on both sides of a Bedrock connection. `crates/axolotl-xbl` obtains credentials (Windows Live device-code OAuth, Xbox device/SISU/XSTS tokens, PlayFab and Minecraft franchise service tokens, per-connection multiplayer tokens); `crates/axelerator/src/token_cache.rs` persists the OAuth token; `crates/jolyne/src/auth` builds client logins and verifies server-side logins. The join state machine that carries the login, including the encryption handshake that proves key possession, is `session-handshake`.

## Requirements

### Requirement: Device-code OAuth flow
`DeviceCodeAuth` SHALL run the Windows Live device-code flow with Minecraft's client ID `0000000048183522` and scope `service::user.auth.xboxlive.com::MBI_SSL`. Polling MUST treat only HTTP 400 `authorization_pending`, `slow_down` and `expired_token` as control responses and MUST never accept a token from a non-success response. Every accepted token MUST have an access token and a positive, representable lifetime.

#### Scenario: Control responses
- **WHEN** a poll is answered HTTP 400 with `authorization_pending` or `slow_down`
- **THEN** `poll` returns `Ok(None)` or `Err(AuthorizationSlowDown)`, and `wait_for_auth` adds 5 s to its interval after a slow-down

#### Scenario: Token-shaped error body
- **WHEN** a poll is answered HTTP 500 with a body containing `access_token` and `refresh_token`
- **THEN** the poll fails with that status and no token

#### Scenario: Lifetime checks
- **WHEN** a response's `expires_in` is `u64::MAX`, or the device code's `expires_in` is 0
- **THEN** the token is rejected, or `wait_for_auth` returns `DeviceCodeExpired` without polling

#### Scenario: Refresh keeps the refresh token
- **WHEN** a refresh response omits `refresh_token`
- **THEN** the returned token keeps the refresh token that was sent

Evidence: crates/axolotl-xbl/src/auth/mod.rs (`MINECRAFT_CLIENT_ID`, `XBL_SCOPE`), crates/axolotl-xbl/src/auth/device_code.rs (`pending_and_slow_down_are_protocol_control_responses`, `non_success_token_body_is_not_accepted`, `overflowing_oauth_lifetime_is_rejected`, `expired_device_code_does_not_poll_network`, `DeviceCodeAuth::refresh_at`)

### Requirement: Xbox token acquisition
`XblTokenClient` SHALL obtain a ProofOfPossession device token, a SISU authorization (title, user and Xbox Live tokens) and per-relying-party XSTS tokens, signing each request with one P-256 proof key. Every service token MUST be valid for more than 60 s and carry a user hash, or acquisition fails. Tokens MUST be cached per relying party until 60 s before expiry, and `invalidate_token` MUST drop only the rejected token.

#### Scenario: One proof key across the flow
- **WHEN** a token for a non-default relying party is acquired against loopback auth servers
- **THEN** NSAL, SISU and the scoped XSTS request are signed with the same proof key, and the signature covers the outgoing headers and body

#### Scenario: Invalid stages
- **WHEN** the device, SISU or XSTS response has a missing, past or near-expiry `NotAfter`, or lacks user claims
- **THEN** acquisition fails instead of caching the token

#### Scenario: Expired tokens are not reused
- **WHEN** a cached token is within 60 s of `expires_at`
- **THEN** `is_valid()` is false and a new token is acquired

Evidence: crates/axolotl-xbl/src/auth/xbl_token.rs (`EXPIRY_BUFFER_SECS`, `get_xbl_token`, `invalidate_token`, `acquisition_http_smoke_uses_nsal_sisu_and_scoped_xsts_with_one_proof_key`, `service_signature_covers_actual_outgoing_extra_headers`, `acquisition_http_rejects_invalid_or_missing_expiry_at_each_stage`, `acquisition_http_rejects_missing_user_claims`, `invalid_service_tokens_are_not_reused`), crates/axolotl-xbl/src/auth/signing.rs, crates/axolotl-xbl/src/auth/nsal.rs

### Requirement: PlayFab and Minecraft service tokens
`PlayFabClient` SHALL accept only HTTP 200 from PlayFab login and the Minecraft session, renew and multiplayer endpoints, and send the caller's game version in session start. It MUST cache a PlayFab ticket per user hash and a service token per device ID, game version and ticket, renew a token within one minute of `validUntil`, start a fresh session when renewal fails, and invalidate only the rejected token. Service-token claims MUST be time-checked, not signature-verified.

#### Scenario: Ticket lifetime
- **WHEN** `login` is called again for the same user hash within 23 h 45 min
- **THEN** the cached ticket is returned without a request

#### Scenario: Account switch
- **WHEN** the ticket changes for the same device and version
- **THEN** the cached service token is not reused

#### Scenario: Claims
- **WHEN** a session token's `pmid` is not a non-nil UUID, or its `exp`, `nbf` or `iat` is outside a 60 s leeway
- **THEN** the session is rejected

#### Scenario: Multiplayer token per key
- **WHEN** `multiplayer_token` is called with two different SPKI keys
- **THEN** each request carries its own base64 `publicKey`, and an empty `signedToken` is an error

Evidence: crates/axolotl-xbl/src/playfab.rs (`SessionResult::decode_claims`, `login_reuses_ticket_until_refresh_margin`, `service_cache_binds_tokens_to_ticket_after_account_switch`, `service_token_renews_before_hard_expiry_and_caches_result`, `rejected_renewal_starts_fresh_session`, `invalidation_does_not_erase_a_newer_token`, `session_transmits_configured_version_and_redacts_errors`, `multiplayer_token_transmits_each_key_and_redacts_failures`, `login_checks_status_before_parsing_and_redacts_errors`)

### Requirement: Credentials never in errors or logs
Credential-bearing values SHALL never appear in an error's `Display` or `Debug`, an authentication object's `Debug`, or a log line. `XblError` MUST retain only an HTTP status, a JSON error category and a static operation name, never a response body or URL, and its `Debug` MUST equal its `Display`. A jolyne server MUST never send an authentication error's text to the peer (see `session-handshake`).

#### Scenario: Remote error bodies
- **WHEN** an auth, PlayFab, discovery or OAuth endpoint answers an error or malformed JSON whose body contains secrets
- **THEN** the formatted error and its `Debug` contain the status and operation but not the secret

#### Scenario: Debug redaction
- **WHEN** an `OAuthToken`, `DeviceCodeResponse`, `XblToken`, PlayFab response or `CachedToken` is formatted with `{:?}`
- **THEN** its tokens and codes print as `[REDACTED]`

Evidence: crates/axolotl-xbl/src/error.rs (`XblError`), crates/axolotl-xbl/tests/auth_tests.rs (`credentials_are_redacted_from_debug`, `malformed_response_error_does_not_disclose_credentials`), crates/axolotl-xbl/src/auth/device_code.rs (`oauth_failure_description_cannot_leak_token`, `malformed_success_response_is_redacted`), crates/axolotl-xbl/src/auth/xbl_token.rs (`acquisition_http_status_errors_never_expose_remote_secrets`, `malformed_claims_do_not_panic_or_leak`), crates/axolotl-xbl/src/playfab.rs (`secret_response_debug_is_redacted`), crates/axolotl-xbl/src/discovery.rs (`discovery_checks_status_and_redacts_malformed_responses`), crates/axelerator/src/token_cache.rs (`cached_debug_redacts_credentials`), crates/jolyne/src/proto/tests.rs (`failure_disconnects_never_send_internal_error_text`)

### Requirement: Private, atomic token cache
`TokenCache` SHALL persist the OAuth token by writing a new temporary file created owner-only before any credential byte is written (Unix mode `0600`; Windows a protected DACL granting only the current user), syncing it and renaming it over the destination. A failed write MUST remove the temporary file and leave the destination intact.

#### Scenario: Replacement
- **WHEN** a token is saved over an existing cache file that had broader permissions
- **THEN** the destination holds complete JSON with owner-only permissions, and a reader of the old file never sees a truncated one

#### Scenario: Failed replacement
- **WHEN** the rename fails
- **THEN** the temporary file is removed and the previous cache file is unchanged

Evidence: crates/axelerator/src/token_cache.rs (`save_securely`, `create_private_file`, `private_permissions_exist_before_credentials_are_written`, `atomic_replacement_preserves_complete_json`, `replacement_corrects_insecure_permissions`, `replacement_has_only_current_user_acl`, `failed_replacement_cleans_temporary_and_preserves_destination`)

### Requirement: Token cache loading
`TokenCache` SHALL start interactive device-code login only when the cache file is absent. A malformed or unreadable cache MUST be an error, not a cache miss; an expired token without a refresh token MUST be an error that leaves the file in place; a failed refresh MUST NOT fall back to interactive login. Loading MUST keep the persisted expiry, and the OAuth token MUST count as expired 10 s before it.

#### Scenario: Malformed cache
- **WHEN** the cache file holds invalid JSON
- **THEN** loading fails with "Failed to parse token cache" and no device-code flow starts

#### Scenario: Expiry margin
- **WHEN** fewer than 10 s remain before `expires_at`
- **THEN** the cached OAuth token is treated as expired

Evidence: crates/axelerator/src/token_cache.rs (`try_load_from_file`, `absent_cache_is_a_cache_miss`, `malformed_cache_is_a_safe_error_not_a_cache_miss`, `cache_read_errors_are_not_cache_misses`, `expired_cache_without_refresh_token_returns_error_and_preserves_file`, `failed_in_memory_refresh_preserves_cache_without_disk_fallback`, `loading_preserves_original_expiry`, `oauth_expiration_uses_ten_second_margin`, `maximum_oauth_lifetime_saturates_without_panicking`)

### Requirement: Login request envelope
The server SHALL decode `LoginPacket.connection_request` as `[i32 LE length][AuthenticationInfo JSON][i32 LE length][client-data JWT]` with checked arithmetic, rejecting negative or oversized lengths, truncation, trailing bytes and invalid UTF-8. `AuthenticationType` MUST be present and one of 0, 1 or 2, and type 0 MUST carry a non-empty `Token`.

#### Scenario: Malformed envelopes
- **WHEN** the payload is empty, has a short prefix, a negative or huge length, a trailing byte or invalid UTF-8, or is any of 20,000 random mutations of a valid one
- **THEN** decoding returns an error and never panics

#### Scenario: Authentication type
- **WHEN** `AuthenticationType` is missing, 99, or 0 without a token
- **THEN** authentication fails with `MissingAuthType`, `UnsupportedAuthType(99)` or `MissingToken`

Evidence: crates/jolyne/src/auth/mod.rs (`decode_login_request`, `authenticate_login_requires_auth_type`, `authenticate_login_rejects_unknown_auth_type`, `authenticate_login_requires_token_for_full`, `login_request_round_trip_preserves_length_prefixed_values`), crates/jolyne/src/auth/security_tests.rs (`malformed_envelopes_never_panic`, `malformed_tokens_and_client_data_never_panic`)

### Requirement: Online trust comes only from published provider keys
In online mode the server SHALL verify the multiplayer `Token` only against keys from Minecraft build discovery: the discovery URL for `GAME_VERSION` names the production issuer, whose `.well-known/openid-configuration` MUST name the same issuer and the `jwks_uri`. Keys or URLs from the login (JWT `x5u`/`x5c` headers) MUST never be trusted. Fetches MUST be HTTPS-only.

#### Scenario: Discovery chain
- **WHEN** the production key source fetches over a stubbed HTTP client
- **THEN** it requests build discovery, then the issuer's OpenID configuration, then its `jwks_uri`, and fails with `InvalidIssuer` when the configuration names another issuer

#### Scenario: Self-signed token
- **WHEN** a token is signed by a key that is not in the trusted JWKS
- **THEN** validation fails

#### Scenario: Malformed JWKS entries
- **WHEN** the JWKS mixes a malformed key with valid ones
- **THEN** the malformed key is skipped and the valid ones verify; a set with no usable key is an error

Evidence: crates/jolyne/src/auth/openid.rs (`MinecraftDiscovery`, `OpenIdVerifier::from_jwks`, `HTTP_CLIENT`), crates/jolyne/src/auth/provider_tests.rs (`minecraft_discovery_follows_build_discovery_to_the_jwks`), crates/jolyne/src/auth/security_tests.rs (`rs256_jwks_validates_provider_tokens`, `self_signed_header_key_is_not_online_trust`, `malformed_jwks_keys_are_skipped_not_fatal`)

### Requirement: Multiplayer token claim validation
A provider token SHALL verify only with a trusted key of the same algorithm (and the same `kid` when the token names one). `iss` MUST equal the discovered issuer, `aud` MUST be `api://auth-minecraft-services/multiplayer`, `exp` MUST be present, `nbf` MUST be checked when present, and the clock leeway MUST be 60 s. The token MUST carry an identity key (`cpk`) and an int64 `xid`.

#### Scenario: Claim failures
- **WHEN** a correctly signed token has a wrong `iss`, a wrong `aud`, an `exp` in the past or an `nbf` an hour ahead
- **THEN** validation fails with `InvalidIssuer`, `InvalidAudience` or `TemporalValidation`

#### Scenario: Missing claims
- **WHEN** `iss`, `aud`, `exp` or `cpk` is absent, or `xid` is missing
- **THEN** validation fails

#### Scenario: Absent nbf
- **WHEN** a valid token has no `nbf`, as real 1.26.52 client tokens do
- **THEN** it is accepted

Evidence: crates/jolyne/src/auth/openid.rs (`OpenIdVerifier::verify_token`, `MOJANG_AUDIENCE`, `CLOCK_SKEW`), crates/jolyne/src/auth/types.rs (`ValidatedIdentity::new`, `authenticated_identity_requires_an_xuid`, `identity_data_rules_match_gophertunnel`), crates/jolyne/src/auth/security_tests.rs (`trusted_signatures_do_not_override_claim_validation`, `online_token_without_xuid_is_not_authenticated`)

### Requirement: Provider key refresh
`OpenIdProvider` SHALL serve logins from a cached key snapshot without holding a lock across HTTP, run at most one fetch at a time and share its outcome, and keep serving cached keys while a refresh is pending or failing until they are too old. A fetch MUST be bounded by a timeout and MUST complete even when the login that started it is cancelled.

#### Scenario: Refresh policy defaults
- **WHEN** `RefreshPolicy::default()` is used
- **THEN** an unknown `kid` refreshes at most once per 30 min, snapshots are revalidated after 1 h, stale keys serve for up to 24 h after failed refreshes, failed fetches back off 30 s, and a fetch times out after 30 s

#### Scenario: Concurrent cold logins
- **WHEN** many logins arrive before any keys are cached
- **THEN** exactly one fetch runs and every login uses its result, or shares its error

#### Scenario: Hung or panicking source
- **WHEN** the key source hangs or panics
- **THEN** logins fail with `ProviderUnavailable` and a later refresh starts a new fetch

Evidence: crates/jolyne/src/auth/openid.rs (`OpenIdProvider`, `RefreshPolicy`), crates/jolyne/src/auth/provider_tests.rs (`concurrent_cold_logins_share_one_fetch`, `stale_keys_keep_serving_while_one_caller_refreshes`, `unknown_kid_refreshes_once_per_interval`, `refresh_failure_keeps_serving_cached_keys_until_max_stale`, `failed_cold_fetch_is_shared_and_backed_off`, `fetch_timeout_bounds_a_hung_source`, `cancelled_login_does_not_abort_or_forget_the_fetch`, `panicking_fetch_frees_the_refresh_slot`)

### Requirement: Client data bound to the identity key
In every mode the client-data JWT SHALL verify as ES384 against the identity key from the token (or legacy chain), and an `IdentityPublicKey` it echoes MUST equal that key. The decoded `ClientData` MUST then pass the gophertunnel validation rules, and its list fields MUST be length-bounded while parsing.

#### Scenario: Foreign signature or echo
- **WHEN** the client data is signed by another key, or echoes another `IdentityPublicKey`
- **THEN** the login is rejected

#### Scenario: Validation rules
- **WHEN** a correctly signed client data has, for example, `DeviceOS` outside 1..=15 or an empty `SkinId`
- **THEN** the login fails with `AuthError::InvalidClientData`

#### Scenario: List bounds
- **WHEN** `AnimatedImageData` has more than 32 entries, `PersonaPieces` or `PieceTintColors` more than 128, or a tint more than 16 colours
- **THEN** parsing fails

Evidence: crates/jolyne/src/auth/openid.rs (`verify_client_data`), crates/jolyne/src/auth/client_data.rs (`ClientData::validate`, `each_gophertunnel_rule_rejects_its_field`, `oversized_lists_are_rejected_while_parsing`, `huge_declared_dimensions_do_not_overflow_or_allocate`), crates/jolyne/src/auth/security_tests.rs (`trusted_provider_binds_client_data_to_cpk`, `signed_but_invalid_client_data_is_rejected`)

### Requirement: Authenticated only after key proof
A provider-verified identity SHALL be `Trust::AwaitingKeyProof` until the connection proves the client holds the token's key, by completing the encryption handshake or through `Transport::proven_peer_key`. `xuid()`, `playfab_id()` and `title_id()` MUST return `None` unless the identity is `Authenticated`. Online mode with encryption off MUST be refused on RakNet; on other transports an unproven identity MUST be downgraded to `Unauthenticated`.

#### Scenario: Handshake completes
- **WHEN** an online login finishes the encryption handshake
- **THEN** the identity becomes `Authenticated` with its XUID and PlayFab ID exposed

#### Scenario: Replayed login
- **WHEN** a valid token is sent by a client that does not hold its key
- **THEN** its handshake reply fails the checksum, the identity stays `AwaitingKeyProof`, and the join times out at the login deadline

#### Scenario: RakNet without encryption
- **WHEN** a RakNet listener is bound with `online_mode` and without `encryption_enabled`
- **THEN** `bind` fails with `AuthError::OnlineModeRequiresEncryption`

#### Scenario: Transport-proven key
- **WHEN** encryption is off on a message transport
- **THEN** the identity is `Authenticated` only if the transport-proven key equals the login's key, else `Unauthenticated`

Evidence: crates/jolyne/src/auth/types.rs (`Trust`, `ValidatedIdentity`), crates/jolyne/src/proto/server.rs (`Authenticated::start_encryption`, `ServerEncryption`), crates/jolyne/src/listener.rs, crates/jolyne/src/proto/tests.rs (`online_identity_is_authenticated_only_after_the_encryption_handshake`, `replayed_online_login_never_authenticates_and_times_out_at_the_login_deadline`, `online_mode_without_encryption_is_refused_on_raknet`, `unencrypted_online_login_trusts_only_a_transport_proven_key`), crates/jolyne/tests/raknet_listener.rs (`online_raknet_listener_requires_encryption`), crates/jolyne/src/auth/security_tests.rs (`online_identity_carries_playfab_and_title_ids`)

### Requirement: Offline mode is never authenticated
With `online_mode` off the server SHALL read the token's claims without signature or expiry checks and mark the identity `Trust::Unauthenticated`; client data MUST still verify against the claimed `cpk`. Authentication types 1 and 2 (self-signed tokens or legacy chains) MUST be accepted only offline with `allow_legacy_auth`, and online mode MUST reject them with `AuthError::LegacyAuthDisabled`.

#### Scenario: Claimed account IDs
- **WHEN** an offline login's token claims an XUID and PlayFab ID
- **THEN** `xuid()`, `playfab_id()` and `title_id()` are `None` and only `claimed_xuid()` returns the claim

#### Scenario: Legacy auth gating
- **WHEN** a type-2 login arrives online (even with `allow_legacy_auth`), or offline with `allow_legacy_auth` off
- **THEN** it fails with `LegacyAuthDisabled`; offline with it on, a self-signed chain is accepted as unauthenticated

#### Scenario: Offline client against online server
- **WHEN** jolyne's offline client logs in to an online server and to an offline one
- **THEN** the online server rejects it and the offline server accepts it unauthenticated

Evidence: crates/jolyne/src/auth/mod.rs (`authenticate_login_with`, `offline_token_login`, `authenticate_login_rejects_legacy_in_online_mode_even_when_legacy_allowed`, `authenticate_login_rejects_legacy_when_disabled`), crates/jolyne/src/auth/legacy.rs (`offline_chain_accepts_padded_jwt_and_is_never_authenticated`), crates/jolyne/src/auth/types.rs (`offline_identity_never_exposes_trusted_account_ids`), crates/jolyne/src/auth/security_tests.rs (`offline_token_is_accepted_but_never_authenticated`, `offline_type_zero_token_is_also_unauthenticated`, `legacy_self_signed_chain_is_offline_only`, `offline_client_login_is_rejected_online_and_accepted_offline`)

### Requirement: Client login construction
jolyne's client SHALL build `LoginCredentials::Online` logins as `AuthenticationType` 0 with a provider-signed multiplayer token issued for this connection's P-384 key, and `Offline` logins as `AuthenticationType` 2 with a self-signed token bound to the client signing key. An empty provider token MUST be rejected before sending, and default client data MUST pass the server's validation with a retail-supported `DeviceOS`.

#### Scenario: Online credentials
- **WHEN** an online login is built
- **THEN** the token source is asked for a token for the connection key's SPKI DER and the request carries that token unchanged; an online jolyne server accepts it

#### Scenario: Offline DeviceOS
- **WHEN** an offline login is built with default client data
- **THEN** `DeviceOS` is 1 (Android), which BDS 1.26.51 accepts

Evidence: crates/jolyne/src/auth/client.rs (`build_login`, `MultiplayerTokenSource`, `online_credentials_send_the_modern_token_for_the_connection_key`, `empty_provider_token_is_rejected_before_sending`, `offline_login_identity_token_binds_the_client_signing_key`, `offline_login_encodes_a_retail_supported_device_os`, `default_client_data_passes_server_validation`, `authenticated_login_uses_full_auth_and_preserves_provider_token`), crates/jolyne/src/auth/security_tests.rs (`online_client_login_is_accepted_by_online_server`)
Verified by: `tools/client-diff` (jolyne's offline client completes `join-idle` and `join-chat` against BDS 1.26.51)
