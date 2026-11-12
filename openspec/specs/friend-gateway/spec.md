# friend-gateway Specification

## Purpose
`crates/axelerator` is the friend-broadcast gateway: a long-running binary that signs in to Xbox Live as a broadcast account, publishes a joinable multiplayer session so the account's friends see the server in their friends list, accepts their NetherNet connections on a mini Bedrock server and immediately transfers each player to the configured target server. Xbox Live, PlayFab, presence, social and MPSD calls are provided by `axolotl-xbl` (see `xbl-auth`); the WebRTC transport and franchise WebSocket signaling are `nethernet`; the Bedrock join sequence the mini server runs before the transfer is `session-handshake` in `jolyne`. The target server itself is out of scope.

## Requirements

### Requirement: Configuration sources and precedence
The gateway SHALL build its `AxeleratorConfig` from defaults, then the TOML file given by `-c/--config` (default `axelerator.toml`), then CLI overrides (`Args`). A missing file MUST mean defaults; any other load failure, unknown keys included, MUST stop startup non-zero. `axelerator init -o <path>` MUST write the example config and exit. Config errors and `Debug` MUST NOT contain the webhook URL or any config value.

#### Scenario: Defaults
- **WHEN** no config file exists and no overrides are given
- **THEN** the target is `127.0.0.1:19132`, the advertised version and protocol are `jolyne::valentine::{GAME_VERSION, PROTOCOL_VERSION}`, the token cache is `token.json`, friend sync runs every 60 s, session refresh every 30 s, auto-unfollow is on, and monitoring, friend expiry, screenshots and Discord are off

#### Scenario: Malformed file
- **WHEN** the config file exists but is not valid TOML for `AxeleratorConfig`
- **THEN** `AxeleratorConfig::load` returns `ConfigError::Parse` with the path, byte span, 1-based line and column, and the parser message with any quoted value removed (naming the key), and no source error
- **AND** the binary exits non-zero with that message instead of running on defaults

#### Scenario: Unknown key
- **WHEN** the file contains a key no config struct declares, such as a misspelt `sever_ip`, at the top level or in any table
- **THEN** loading fails with ``unknown field `sever_ip` `` and its line and column (every config struct uses `deny_unknown_fields`)

#### Scenario: Documented configs load
- **WHEN** the output of `axelerator init` or a TOML config shown in `deploy/DEPLOYMENT.md` is loaded
- **THEN** it parses under the strict rules and its values are applied

#### Scenario: Redacted webhook
- **WHEN** a config with a Discord webhook URL is formatted with `Debug`
- **THEN** neither the URL nor `/api/webhooks/` appears in the output

Evidence: crates/axelerator/src/main.rs (`Args`, `Commands::Init`), crates/axelerator/src/config.rs (`AxeleratorConfig::default`, `load`, `example_toml`, `ConfigError::Parse`, `redact_toml_message`, tests `config_debug_redacts_webhook_credentials`, `malformed_config_errors_do_not_retain_input`, `parse_errors_name_the_key_and_position`, `unknown_keys_are_rejected`, `generated_example_loads`, `documented_configs_load`, `documented_sample_values_are_applied`), crates/axelerator/deploy/DEPLOYMENT.md

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
A bind failure SHALL be classified by its text: `401`/`403`/`Unauthorized`/`Forbidden` is an auth error, `Invalid URL`/`missing host` fatal, the rest retried with backoff from 1 s doubling to 5 min plus up to 25 % jitter. After 10 consecutive failures it MUST return `ListenerClosed`; only a run that accepted a connection or stayed bound 60 s resets the count. On `ListenerClosed` or an auth error the session MUST restart it with a fresh Minecraft token.

#### Scenario: Transient bind failure
- **WHEN** binding the listener fails with a network error
- **THEN** "Reconnecting transfer server..." is logged and the bind is retried after the backoff, and a shutdown during the backoff returns `ShutdownReason::Requested`

#### Scenario: Listener closes straight after binding
- **WHEN** the franchise WebSocket accepts the upgrade and then closes at once, every time
- **THEN** each listener run ends on its non-recoverable accept error instead of spinning, the count is not reset, and after 10 consecutive failures `run_transfer_server` returns `ShutdownReason::ListenerClosed`
- **AND** the session fetches a fresh Minecraft token, re-PUTs the session and starts a new transfer server

#### Scenario: Expired service token
- **WHEN** the franchise WebSocket handshake fails with HTTP 401
- **THEN** the transfer server returns `ShutdownReason::AuthenticationError` without retrying, and the session invalidates the service token, force-refreshes the Xbox token and restarts it with a freshly obtained Minecraft token
- **AND** a failed refresh re-enters that branch after 5 s, counting towards the limit of 5 refresh attempts, and the finished transfer task is never polled again

Evidence: crates/axelerator/src/transfer.rs (`run_transfer_server`, `ListenerRun::was_healthy`, `RetryState`, `accept_error_outcome`, `classify_error`, `ShutdownReason`, `MAX_RETRY_ATTEMPTS`, `INITIAL_BACKOFF_MS`, `MAX_BACKOFF_MS`, `MIN_HEALTHY_UPTIME`, tests `closed_listener_ends_the_accept_loop`, `closed_signaling_socket_ends_the_listener`, `bind_then_immediate_close_exhausts_the_retry_budget`, `healthy_session_needs_uptime_or_a_connection`, `retry_budget_resets_after_a_successful_bind`, `consecutive_failures_exhaust_the_budget`, `backoff_doubles_up_to_the_cap`, `handshake_errors_are_classified_by_status`), crates/axelerator/src/session.rs (`run_signaling_loop`, `TransferTask`, `TOKEN_REFRESH_RETRY_DELAY`, tests `finished_transfer_task_is_not_polled_again`, `failed_refresh_is_retried_through_the_same_branch`)

### Requirement: Session upkeep
While running, the gateway SHALL keep the advertisement alive: presence is refreshed at the heartbeat interval the service returns (`presence_heartbeat`, default 300 s, when it returns 0), with backoff from 5 s doubling to 300 s after failures; the session is re-PUT every `session_refresh_interval` (raised to at least 20 s, 0 disables); and the session's RTA connection ID is updated within 5 s of an RTA reconnect. Long-running calls MUST take a fresh token from the cache.

#### Scenario: Member limit
- **WHEN** a session refresh reports 28 or more members
- **THEN** a new session with a new session ID and handle is created that keeps the same NetherNet ID and connection ID, so the transfer server keeps running

#### Scenario: RTA reconnect
- **WHEN** RTA reconnects with a new connection ID
- **THEN** the session is re-PUT with that ID, retrying once with a force-refreshed token on 401/403

#### Scenario: Omitted heartbeat
- **WHEN** presence returns a heartbeat of 0
- **THEN** the next refresh is scheduled `presence_heartbeat` seconds later (300 by default, at least 1), not immediately
- **AND** a non-zero heartbeat from the service always wins over the configured value

Evidence: crates/axelerator/src/session.rs (`run_signaling_loop`, `run_rta_connection_sync`, `presence_interval`, `PRESENCE_INITIAL_BACKOFF_SECS`, `PRESENCE_MAX_BACKOFF_SECS`, tests `omitted_heartbeat_does_not_create_a_busy_refresh_loop`, `configured_presence_heartbeat_is_the_fallback`), crates/axelerator/src/config.rs (`presence_heartbeat`)

### Requirement: Friend list management
With `auto_accept_friends` (default `true`) the gateway SHALL accept pending friend requests and invite each new friend at startup, on RTA `IncomingFriendRequestCountChanged` and on RTA resync; with it off they MUST be left alone. A periodic sync (every `friend_sync_interval`, at least 20 s, 0 disables) MUST follow back unfollowed followers, skipping guest and already processed XUIDs (at most 50000 kept), and with `auto_unfollow` unfollow at most 10 one-way follows per cycle.

#### Scenario: Auto-accept turned off
- **WHEN** the config sets `auto_accept_friends = false`
- **THEN** no pending friend request is accepted at startup, on RTA notifications or on resync, while follow-back of followers still runs every `friend_sync_interval`

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
- **AND** a friend who joins after the expiry check read their last-seen time is neither unfriended nor dropped from the history

Evidence: crates/axelerator/src/session.rs (`accept_friends_and_invite`, `run_periodic_friend_sync`, `sync_followers`, `run_friend_expiry_check`, `expire_inactive`, `ProcessedXuids`, `is_rate_limited_error`, `is_guest_account`, `FRIEND_SYNC_MIN_INTERVAL_SECS`, tests `rate_limit_classification_uses_status_not_remote_strings`, `join_during_expiry_keeps_the_fresh_entry`, `join_before_unfriend_skips_the_removal`), crates/axelerator/src/config.rs (`auto_accept_friends`, test `friend_requests_are_accepted_by_default`), crates/axelerator/src/player_history.rs (`PlayerHistory::path_from_token_cache`, `get_expired`, `remove_if_unchanged`)

### Requirement: Player history persistence
With friend expiry enabled, `run` SHALL load `player_history.json` once and share that one history between the transfer server and the expiry task. A missing file MUST mean an empty history; a corrupt or unreadable file MUST stop startup and be left untouched. Saves MUST write a synced temporary file and rename it over the old one, outside the data lock, one save at a time so an older snapshot never replaces a newer one.

#### Scenario: Corrupt history
- **WHEN** `player_history.json` exists but does not parse
- **THEN** `run` fails before anything is advertised and the file's bytes are unchanged

#### Scenario: Join during a save
- **WHEN** a player's join is recorded while a save is writing
- **THEN** the join stays pending and is written by the next save, and concurrent saves keep every join

Evidence: crates/axelerator/src/player_history.rs (`PlayerHistory::load`, `save_if_dirty`, `write_atomically`, tests `corrupt_history_is_an_error_and_left_untouched`, `missing_history_starts_empty`, `save_replaces_the_file_atomically`, `failed_save_keeps_the_destination_and_cleans_up`, `change_during_a_save_stays_pending`, `concurrent_saves_keep_every_join`, `clones_share_one_history`), crates/axelerator/src/session.rs (`Axelerator::run`)

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
Discord notifications SHALL be sent only when `discord.enabled` is set and a webhook URL is configured, and each kind (startup, shutdown, auth error, crash, tampering) only when its `notify_on_*` flag is set. Each kind MUST have its own rate limit of `rate_limit_secs` (default 60), whose slot is taken when the notification is raised, before it is posted. Sending MUST happen in a background task so a slow webhook never delays the gateway, and shutdown MUST wait up to 5 s for posts in flight.

#### Scenario: Disabled by default
- **WHEN** the default configuration is used
- **THEN** no webhook request is made

#### Scenario: Rate limit per kind
- **WHEN** a notification of one kind was raised less than `rate_limit_secs` ago, whether or not its post succeeded, and another of the same kind is raised
- **THEN** the new one is dropped, while a notification of a different kind (for example a crash after the startup notice) is still posted

Evidence: crates/axelerator/src/discord.rs (`NotificationKind`, `RateLimiter::try_acquire`, `DiscordNotifier::is_enabled`, `notify`, `notify_startup`, `notify_shutdown`, `notify_auth_error`, `notify_crash`, `notify_tampering`, `flush`, tests `rate_limit_is_per_kind`, `slot_is_taken_when_raised_not_after_posting`, `disabled_notifier_posts_nothing`, `crash_after_startup_is_posted`, `failing_webhook_is_still_rate_limited`), crates/axelerator/src/config.rs (`DiscordWebhookConfig`), crates/axelerator/src/session.rs (`DISCORD_FLUSH_TIMEOUT`)

### Requirement: Shutdown and exit status
Shutdown SHALL be a durable request (`Axelerator::shutdown`) that is never lost. On Ctrl+C or SIGTERM the binary MUST request it and let `run` finish its cleanup (RTA disconnect, history save, presence inactive, Discord notice) for up to 20 s; a second signal MUST exit at once. Only a requested shutdown exits 0; every error MUST exit non-zero. The MPSD session is not deleted.

#### Scenario: Graceful stop
- **WHEN** SIGTERM arrives while the gateway is running
- **THEN** `run` completes its cleanup and the process exits 0

#### Scenario: Second signal
- **WHEN** a second signal arrives while cleanup is still running
- **THEN** the process exits at once with an error saying cleanup was skipped, and logs the skipped steps

#### Scenario: Fatal error under systemd
- **WHEN** the gateway stops on an error that recovery cannot fix
- **THEN** it exits non-zero, and the shipped unit (`Restart=on-failure`, `RestartPreventExitStatus=0`, `StartLimitIntervalSec=0`, `RestartSteps=`/`RestartMaxDelaySec=` backing off from 5 s to 5 min) keeps restarting it

Evidence: crates/axelerator/src/main.rs (`run_until_signal`, `shutdown_signal`, `SHUTDOWN_GRACE`, tests `signal_lets_run_finish_its_cleanup`, `stuck_cleanup_is_an_error_after_the_grace_period`, `second_signal_skips_cleanup`, `run_errors_without_a_signal_are_returned`), crates/axelerator/src/session.rs (`Axelerator::shutdown`, `transfer_exit`, tests `shutdown_request_is_never_lost`, `unrecoverable_transfer_exits_are_errors`), crates/axelerator/deploy/axelerator.service, crates/axelerator/tests/deploy_unit.rs (`unit_keeps_restarting_with_backoff`)
