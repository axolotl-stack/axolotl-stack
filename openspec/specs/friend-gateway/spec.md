# friend-gateway Specification

## Purpose
`crates/axelerator` is the friend-broadcast gateway: a long-running binary that signs in to Xbox Live as a broadcast account, publishes a joinable multiplayer session so the account's friends see the server in their friends list, accepts their NetherNet connections on a mini Bedrock server and immediately transfers each player to the configured target server. Xbox Live, PlayFab, presence, social and MPSD calls are provided by `axolotl-xbl` (see `xbl-auth`); the WebRTC transport and franchise WebSocket signaling are `nethernet`; the Bedrock join sequence the mini server runs before the transfer is `session-handshake` in `jolyne`. The target server itself is out of scope.

## Requirements

### Requirement: Configuration sources and precedence
The gateway SHALL build its `AxeleratorConfig` from defaults, then the TOML file given by `-c/--config` (default `axelerator.toml`), then CLI overrides (`--server-ip`, `--server-port`, `--host-name`, `--token-path`, `--monitor`, `--auto-block`, `--log-level`, screenshot flags). A missing file MUST mean defaults. `axelerator init -o <path>` MUST write `AxeleratorConfig::example_toml()` and exit. Config errors and `Debug` output MUST NOT contain the Discord webhook URL or the file contents.

#### Scenario: Defaults
- **WHEN** no config file exists and no overrides are given
- **THEN** the target is `127.0.0.1:19132`, the advertised version and protocol are `jolyne::valentine::{GAME_VERSION, PROTOCOL_VERSION}`, the token cache is `token.json`, friend sync runs every 60 s, session refresh every 30 s, auto-unfollow is on, and monitoring, friend expiry, screenshots and Discord are off

#### Scenario: Malformed file
- **WHEN** the config file exists but is not valid TOML for `AxeleratorConfig`
- **THEN** `AxeleratorConfig::load` returns `ConfigError::Parse` holding only the path and byte span, with no source error
- **AND** the binary prints a warning and continues with defaults (`load_or_default`)

#### Scenario: Redacted webhook
- **WHEN** a config with a Discord webhook URL is formatted with `Debug`
- **THEN** neither the URL nor `/api/webhooks/` appears in the output

Evidence: crates/axelerator/src/main.rs (`Args`, `Commands::Init`), crates/axelerator/src/config.rs (`AxeleratorConfig::default`, `load`, `load_or_default`, `example_toml`, tests `config_debug_redacts_webhook_credentials`, `malformed_config_errors_do_not_retain_input`)

### Requirement: Xbox Live credential cache
The gateway SHALL authenticate the broadcast account through `TokenCache` at `token_cache_path`, starting the interactive device-code flow only when no cache file exists. A malformed or unreadable cache, or an expired token without a refresh token, MUST be an error that leaves the file unchanged. Saves MUST be atomic and owner-only before credentials are written; errors and `Debug` MUST redact tokens.

#### Scenario: Cache states
- **WHEN** the cache file is absent, malformed, a directory, or holds an expired token with no refresh token
- **THEN** loading returns no token (first login), a "parse token cache" error, a "read token cache" error, and an error preserving the file, respectively

#### Scenario: Atomic private replacement
- **WHEN** a token is saved over an existing world-readable cache
- **THEN** it is written to a temporary file, synced and renamed, so readers see either the complete old or complete new JSON
- **AND** the new file is owner-only (Unix `0600`, Windows a DACL for the current user only), and a failed replacement removes its temporary file and keeps the old cache

#### Scenario: Expiry margin and refresh eligibility
- **WHEN** a cached OAuth token's expiry is checked
- **THEN** it counts as expired from 10 s before `expires_at`, refresh is allowed whenever a refresh token is present, and loading keeps the original expiry

Evidence: crates/axelerator/src/token_cache.rs (`TokenCache`, `save_securely`, `create_private_file`, tests `absent_cache_is_a_cache_miss`, `malformed_cache_is_a_safe_error_not_a_cache_miss`, `cache_read_errors_are_not_cache_misses`, `expired_cache_without_refresh_token_returns_error_and_preserves_file`, `failed_in_memory_refresh_preserves_cache_without_disk_fallback`, `private_permissions_exist_before_credentials_are_written`, `failed_replacement_cleans_temporary_and_preserves_destination`, `atomic_replacement_preserves_complete_json`, `replacement_corrects_insecure_permissions`, `replacement_has_only_current_user_acl`, `oauth_expiration_uses_ten_second_margin`, `refresh_eligibility_uses_presence_not_assumed_lifetime`, `loading_preserves_original_expiry`, `cached_debug_redacts_credentials`), crates/axelerator/README.md (Token cache security)
Verified by: `cargo test -p axelerator`

### Requirement: Session advertisement startup order
`Axelerator::run` SHALL advertise only after each prerequisite succeeds, in order: Xbox Live authentication, an RTA connection ID, presence set active, MPSD session creation with that connection ID, then a session handle. Any failure in this sequence MUST end `run` with an error. The session MUST advertise NetherNet connection type 3, `followed` join/read restriction, `joinable_by_friends`, and the configured names, version and protocol.

#### Scenario: Published session
- **WHEN** startup completes
- **THEN** the session's `SupportedConnections` holds one connection of type 3 with the session's random `nethernet_id`, `MemberCount` is `max(display_players, 1)` and `MaxMemberCount` is `max_players`
- **AND** the log reads "Session created - server is now visible to friends!" and the Discord startup notification (if enabled) is sent

#### Scenario: RTA never connects
- **WHEN** no authenticated RTA connection ID can be obtained
- **THEN** `run` fails with "Failed to establish an authenticated RTA connection" and no session is created

Evidence: crates/axelerator/src/session.rs (`Axelerator::run`, `create_session_info`), crates/axolotl-xbl/src/session.rs (`ExpandedSessionInfo::new`, `CreateSessionRequest::from_session`, `Connection::new`)

### Requirement: Mini NetherNet server
The gateway SHALL discover service endpoints for its configured version, obtain a Minecraft service token by PlayFab login and session start with that version, and bind a `jolyne` NetherNet listener on the franchise WebSocket for the session's NetherNet ID (`BedrockListener::nethernet().xbox(...)`) with `NetherNetListenerConfig::default()`, so joining clients need no `a=identity`. Signal monitoring MUST be enabled only with `monitor_tampering`.

#### Scenario: Signaling URL
- **WHEN** endpoints are discovered
- **THEN** the listener connects to `<signaling service URI>/ws/v1.0/signaling/<nethernet_id>` with the Minecraft token as `Authorization`

#### Scenario: Identityless friend
- **WHEN** a friend's offer arrives without `a=identity` over franchise signaling
- **THEN** the listener answers it and the connection proceeds

Evidence: crates/axelerator/src/session.rs (`run_signaling_loop`, `get_fresh_mc_token`), crates/axelerator/src/transfer.rs (`run_transfer_server_inner`), crates/jolyne/src/listener.rs (`NetherNetSignaling::Xbox` bind), crates/axolotl-xbl/src/discovery.rs (`websocket_url`), crates/tokio-nethernet/src/xbox_signaling.rs (`requires_identity`)

### Requirement: Joining player is transferred
For each accepted connection the gateway SHALL run the `jolyne` server join sequence (`accept_join_sequence` with a default `WorldTemplate` and one random P-384 server key per listener bind) and then send exactly one `TransferPacket` with `server_address` set to `server_ip` verbatim (IP, domain or other string), `server_port`, and `reload_world` false. It MUST then wait 500 ms and close the stream. Each connection MUST run in its own task so a slow or failing join does not block accepts.

#### Scenario: Successful join
- **WHEN** a friend completes the join sequence
- **THEN** the client receives the transfer to `server_ip:server_port`, the connection is closed about 500 ms later, and `transfers_successful` is incremented
- **AND** with friend expiry enabled, the player's XUID last-seen time is updated

#### Scenario: Failed join or send
- **WHEN** the join sequence fails, or the transfer packet cannot be sent
- **THEN** `handshakes_failed` or `transfers_failed` is incremented and other connections are unaffected

Evidence: crates/axelerator/src/transfer.rs (`run_transfer_server_inner`, `handle_transfer_connection`, `send_transfer_packet`, `TransferStats`)

### Requirement: Transfer server recovery
A transfer-server bind failure SHALL be classified by its text: `401`, `403`, `Unauthorized` or `Forbidden` is an authentication error, `Invalid URL` or `missing host` is fatal, anything else is retried. Retries MUST back off from 1 s, doubling to at most 5 min, plus up to 25 % jitter, and stop after 10 attempts. On an authentication error the gateway MUST invalidate the Minecraft service token, force-refresh the Xbox token, fetch a new Minecraft token and restart the transfer server.

#### Scenario: Transient bind failure
- **WHEN** binding the listener fails with a network error
- **THEN** "Reconnecting transfer server..." is logged and the bind is retried after the backoff, and a shutdown during the backoff returns `ShutdownReason::Requested`

#### Scenario: Expired service token
- **WHEN** the franchise WebSocket handshake fails with HTTP 401
- **THEN** the transfer server returns `ShutdownReason::AuthenticationError` without retrying, and the session restarts it with a freshly obtained Minecraft token

Evidence: crates/axelerator/src/transfer.rs (`run_transfer_server`, `classify_error`, `ShutdownReason`, `MAX_RETRY_ATTEMPTS`, `INITIAL_BACKOFF_MS`, `MAX_BACKOFF_MS`), crates/axelerator/src/session.rs (`run_signaling_loop`)

### Requirement: Session upkeep
While running, the gateway SHALL keep the advertisement alive: presence is refreshed at the heartbeat interval the service returns (300 s when it returns 0), with backoff from 5 s doubling to 300 s after failures; the session is re-PUT every `session_refresh_interval` (raised to at least 20 s, 0 disables); and the session's RTA connection ID is updated within 5 s of an RTA reconnect. Long-running calls MUST take a fresh token from the cache.

#### Scenario: Member limit
- **WHEN** a session refresh reports 28 or more members
- **THEN** a new session with a new session ID and handle is created that keeps the same NetherNet ID and connection ID, so the transfer server keeps running

#### Scenario: RTA reconnect
- **WHEN** RTA reconnects with a new connection ID
- **THEN** the session is re-PUT with that ID, retrying once with a force-refreshed token on 401/403

#### Scenario: Omitted heartbeat
- **WHEN** presence returns a heartbeat of 0
- **THEN** the next refresh is scheduled 300 s later, not immediately

Evidence: crates/axelerator/src/session.rs (`run_signaling_loop`, `run_rta_connection_sync`, `presence_interval`, `PRESENCE_INITIAL_BACKOFF_SECS`, `PRESENCE_MAX_BACKOFF_SECS`, test `omitted_heartbeat_does_not_create_a_busy_refresh_loop`)

### Requirement: Friend list management
The gateway SHALL accept every incoming friend request and send each new friend a game invite, on startup, on an RTA `IncomingFriendRequestCountChanged` notification and on an RTA resync. A periodic sync (every `friend_sync_interval`, at least 20 s, 0 disables) MUST follow back followers it does not follow, skipping guest XUIDs and XUIDs already processed (at most 50000 remembered), and, with `auto_unfollow`, unfollow at most 10 one-way follows per cycle.

#### Scenario: Rate limiting
- **WHEN** a social call fails with HTTP 429 or `XblError::RateLimited`
- **THEN** sync pauses for the server's `Retry-After`, or 60 s without one, and unfollows are skipped for that cycle
- **AND** an error whose text merely mentions a rate limit is not treated as one

#### Scenario: Token expiry during sync
- **WHEN** a friends or invite call fails with an authentication error
- **THEN** the Xbox token is force-refreshed and that call retried once

#### Scenario: Friend expiry (opt-in)
- **WHEN** `friend_expiry.enabled` is set
- **THEN** current followers are seeded as seen at startup in `player_history.json` beside the token cache, and every `check_interval` (at least 60 s) friends not seen for `days` days are removed, stopping the cycle at the first rate-limit error

Evidence: crates/axelerator/src/session.rs (`accept_friends_and_invite`, `run_periodic_friend_sync`, `sync_followers`, `run_friend_expiry_check`, `ProcessedXuids`, `is_rate_limited_error`, `is_guest_account`, `FRIEND_SYNC_MIN_INTERVAL_SECS`, test `rate_limit_classification_uses_status_not_remote_strings`), crates/axelerator/src/player_history.rs (`PlayerHistory::path_from_token_cache`, `get_expired`)

### Requirement: Tampering and hijack monitoring
With `monitor_tampering`, the gateway SHALL check every `monitor_interval` seconds whether the session or its handle was modified; on tampering it MUST log the changed fields, notify Discord and re-PUT the session, and when its handle is gone it MUST recreate it. With `auto_block_attackers`, an unknown member found by the check MUST be force-removed as a follower, remembering at most 1000 blocked XUIDs.

#### Scenario: Deleted handle
- **WHEN** the hijack check reports that the gateway's handle no longer exists
- **THEN** a new handle is created and used for later checks

#### Scenario: Unknown member with auto-block
- **WHEN** tampering reports `unknown member joined: <xuid>` and auto-block is on
- **THEN** that XUID is force-removed once and not acted on again while remembered

Evidence: crates/axelerator/src/session.rs (`run_signaling_loop` monitoring block, `MAX_BLOCKED_XUIDS`), crates/axelerator/src/transfer.rs (signal monitoring in `run_transfer_server_inner`)

### Requirement: Operator notifications
Discord notifications SHALL be sent only when `discord.enabled` is set and a webhook URL is configured, and each kind (startup, shutdown, auth error, crash, tampering) only when its `notify_on_*` flag is set. One rate limit of `rate_limit_secs` (default 60) MUST apply across all kinds, measured from the last successfully posted notification. Sending MUST happen in a background task so a slow webhook never delays the gateway.

#### Scenario: Disabled by default
- **WHEN** the default configuration is used
- **THEN** no webhook request is made

#### Scenario: Rate limit after a post
- **WHEN** a notification was posted successfully less than `rate_limit_secs` ago and another of any kind is raised
- **THEN** the new one is dropped

Evidence: crates/axelerator/src/discord.rs (`DiscordNotifier::is_enabled`, `notify`, `notify_startup`, `notify_shutdown`, `notify_auth_error`, `notify_crash`, `notify_tampering`), crates/axelerator/src/config.rs (`DiscordWebhookConfig`)
