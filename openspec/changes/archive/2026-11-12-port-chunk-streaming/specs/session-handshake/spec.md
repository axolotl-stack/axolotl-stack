## MODIFIED Requirements

### Requirement: Outbound batching and backpressure
Outbound packets SHALL be framed into a pending batch sealed by the `FlushMode` (`Immediate` default, `Manual`, `Interval`) and always sealed at `max_packets_per_outbound_batch` or `max_pending_bytes`. Sealed bytes awaiting the transport MUST stay within `max_queued_bytes`; a batch that would exceed it MUST be dropped with `ProtocolError::WriteBufferFull` without advancing the cipher. A failed encode MUST leave earlier packets intact.

#### Scenario: Default bounds
- **WHEN** `ConnectionOptions::default()` is used
- **THEN** batches auto-seal at 812 packets or 256 KiB pending (well under the 512 split parts a RakNet peer reassembles), and sealed output is capped at 64 MiB

#### Scenario: Interval flush
- **WHEN** packets are queued under `FlushMode::Interval(50 ms)`
- **THEN** they are sealed together once 50 ms have passed since the first one, and not before

#### Scenario: Refused batch keeps the cipher in sync
- **WHEN** an encrypted connection refuses a batch with `WriteBufferFull`
- **THEN** batches sent afterwards still decrypt and verify at the peer

#### Scenario: Join groups leave as single batches
- **WHEN** the server sends the `StartGame` group, or answers `RequestChunkRadius`, under `FlushMode::Immediate`
- **THEN** the group, and `ChunkRadiusUpdated` with `PlayStatus(PlayerSpawn)`, each leave as one batch

Evidence: crates/jolyne/src/config.rs (`ConnectionOptions`, `FlushMode`), crates/jolyne/docs/ARCHITECTURE.md, crates/jolyne/src/proto/conn.rs (`flush_modes_immediate_manual_interval_and_auto_seal_limits`, `refused_batch_leaves_the_cipher_in_sync`, `failed_encode_keeps_the_pending_batch_intact`, `switching_to_interval_arms_the_timer_for_pending_packets`, `send_batch_splits_at_the_limit_with_the_requested_reliability`), crates/jolyne/src/proto/tests.rs (`start_game_and_spawn_answers_leave_as_single_batches`), crates/jolyne/src/stream/tests.rs (`manual_and_interval_flush_batch_packets`, `sends_wait_for_transport_capacity`)

### Requirement: Spawn completion
After `StartGame` the server SHALL answer the first `RequestChunkRadius` (at least 1, else `InvalidChunkRadius`) with `ChunkRadiusUpdated` carrying `WorldJoinParams::chunk_radius`, else the request capped at `max_chunk_radius` when set. It MUST complete at a `SetLocalPlayerAsInitialized` with the `StartGame` runtime ID (else `RuntimeIdMismatch`); one sent before the radius request MUST be held. A streamed spawn MUST hand over at the radius request.

#### Scenario: Server override
- **WHEN** the client requests radius 16 and the join params set radius 4
- **THEN** both sessions report chunk radius 4; a request of 0 fails with `InvalidChunkRadius`

#### Scenario: Radius limit
- **WHEN** the client requests radius 16 and the join params set no radius but a limit of 12, and then of 32
- **THEN** both sessions report chunk radius 12, and then 16

#### Scenario: Plain spawn announces no terrain
- **WHEN** `start_game` is used
- **THEN** the radius answer is `ChunkRadiusUpdated` plus `PlayStatus(PlayerSpawn)` with no `NetworkChunkPublisherUpdate`

#### Scenario: Streamed spawn
- **WHEN** `start_game_streamed` is used
- **THEN** the radius answer is `ChunkRadiusUpdated` only, the session reports `is_initialized() == false`, and play receives fail with `RuntimeIdMismatch` or `Timeout(Spawn)` until a matching `SetLocalPlayerAsInitialized` arrives

#### Scenario: Initialize before the radius request
- **WHEN** a client sends a matching `SetLocalPlayerAsInitialized` before its first `RequestChunkRadius`
- **THEN** the server holds it and still answers the radius (`ChunkRadiusUpdated`, plus `PlayStatus(PlayerSpawn)` for `start_game`), then completes the spawn with the radius known; a streamed spawn hands over a session that is already initialized

#### Scenario: Radius never requested
- **WHEN** a client sends `SetLocalPlayerAsInitialized` and never sends `RequestChunkRadius`
- **THEN** the spawn fails with `Timeout(Spawn)` at the spawn deadline

#### Scenario: Client spawn checks
- **WHEN** a client receives a `ChunkRadiusUpdated` below 1, or a second `StartGame` with a different runtime ID
- **THEN** it fails with `InvalidChunkRadius` or `ConflictingStartGame`

Evidence: crates/jolyne/src/proto/server.rs (`ServerSpawn`), crates/jolyne/src/world.rs (`WorldJoinParams`), crates/jolyne/src/proto/session.rs (`Session::next_packet`), crates/jolyne/src/proto/tests.rs (`chunk_radius_requires_one_and_honours_the_server_override`, `chunk_radius_limit_caps_the_granted_request`, `runtime_id_mismatch_is_rejected`, `spawn_does_not_announce_terrain_it_does_not_send`, `streamed_spawn_lets_the_application_deliver_the_view_first`, `streamed_spawn_keeps_the_spawn_deadline_and_runtime_id_check`, `early_initialize_still_gets_the_spawn_answer`, `streamed_early_initialize_hands_over_an_initialized_session`, `client_rejects_a_chunk_radius_below_one`, `client_rejects_a_conflicting_start_game`), crates/jolyne/docs/LOGIN_SEQUENCE.md, crates/jolyne/src/stream/tests.rs (`streamed_spawn_completes_on_the_play_receive_path`, `streamed_spawn_times_out_on_the_play_receive_path`)
