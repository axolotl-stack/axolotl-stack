# session-handshake Specification

## Purpose
`crates/jolyne` is the Bedrock protocol engine: a sans-IO core (`jolyne::proto`) that turns datagrams and instants into packets, phase transitions and sealed batches, and an async driver (`jolyne::stream`, `jolyne::listener`) that runs the core over a `Transport` on Tokio. This spec covers the server and client join state machines, batch framing, compression, encryption, limits, deadlines and disconnect behaviour. Packet byte layouts belong to `protocol-codec` (checked by `tools/wire-conformance`), the RakNet transport to `raknet`, and login verification and Xbox tokens to `xbl-auth`.

## Requirements

### Requirement: Sans-IO core with explicit time
The `jolyne::proto` core SHALL perform no I/O, spawn no tasks and read no clock: input MUST arrive as `conn::Datagram`s, time MUST be passed as `now: Instant`, and output MUST be queued `conn::Transmit`s drained with `poll_transmit`. Every join phase MUST consume itself in `advance(now)` and return `Step::Wait`, `Step::Next` or a `Failure` that still owns the connection.

#### Scenario: Server and client join without a transport
- **WHEN** a server core and a client core are joined by passing each side's transmits to the other's `handle_datagram`
- **THEN** both reach a play `Session` with the same runtime ID and chunk radius, with no socket and no runtime clock involved

#### Scenario: A failure keeps the connection
- **WHEN** a phase fails
- **THEN** the returned `Failure` carries the error and the failed phase, which still owns its `Connection` so the caller can queue a `Disconnect` or `PlayStatus`

Evidence: crates/jolyne/src/proto/phase.rs (`Phase::advance`, `Failure`), crates/jolyne/src/proto/tests.rs (`join_sequence_follows_the_bds_capture`), crates/jolyne/examples/06_sans_io.rs, crates/jolyne/src/stream/tests.rs (`failed_step_keeps_the_connection_and_sends_a_reason`)

### Requirement: Server join order
The server SHALL accept packets only in the order `RequestNetworkSettings`, `Login`, (caller verification and admission), `ClientToServerHandshake` when encryption is on, the resource pack exchange, then `RequestChunkRadius` and `SetLocalPlayerAsInitialized`. Any other packet in the network-settings or login phase MUST fail the join with `ProtocolError::UnexpectedPacket`; later phases defer it (see the deferral requirement).

#### Scenario: Server packets follow the BDS capture
- **WHEN** an offline server core completes a join with custom dimension data
- **THEN** the packets it sends through `PlayStatus(PlayerSpawn)` (apart from `DimensionData`) are an order-preserving subsequence of the committed BDS 1.26.51 trace `bds-1.26.51-join-s2c.txt`
- **AND** `DimensionData` is immediately followed by `JigsawStructureData`, and the structure, `StartGame`, `ItemRegistry` and definition packets fall in one BDS batch

#### Scenario: Post-encryption packets match BDS bytes
- **WHEN** the server finishes the encryption handshake and offers no packs
- **THEN** it sends `PlayStatus(LoginSuccess)`, `ResourcePacksInfo` (nil template UUID, version `0.0.0`) and `ResourcePackStack` (base game version `1.26.51`) with exactly the BDS bodies

#### Scenario: Wrong first packet
- **WHEN** the first packet a server receives is not `RequestNetworkSettings`
- **THEN** the phase fails with `ProtocolError::UnexpectedPacket` naming `HandshakePhase::NetworkSettings` and the packet

Evidence: crates/jolyne/src/proto/server.rs, crates/jolyne/src/proto/phase.rs (`unexpected`), crates/jolyne/src/proto/testdata/bds-1.26.51-join-s2c.txt, crates/jolyne/src/proto/tests.rs (`join_sequence_follows_the_bds_capture`, `post_encryption_packets_match_bds_bytes`), docs/parity/protocol.toml (`NetworkSettingsPacket`, `StartGamePacket` rows)
Verified by: `tools/client-diff` (jolyne and gophertunnel clients complete `join-idle` and `join-chat` against BDS 1.26.51)

### Requirement: Game packets before StartGame
The server SHALL send the application's `WorldJoinParams::before_start_game` packets first in the `StartGame` batch, in their given order, before `DimensionData` and `JigsawStructureData`; jolyne MUST NOT inspect or reorder them.

#### Scenario: Pre-StartGame packets open the StartGame batch
- **WHEN** the application passes `LevelEventGeneric`, `PlayerList` and `SyncWorldClocks` in `before_start_game`
- **THEN** the server sends them in that order directly before `JigsawStructureData`, in the same single batch as `StartGame`, and the packets still follow the BDS trace

Evidence: crates/jolyne/src/proto/server.rs (`send_start_game`), crates/jolyne/src/world.rs (`WorldJoinParams`), crates/jolyne/src/proto/tests.rs (`game_packets_can_precede_start_game_in_its_batch`), docs/decisions/0044-send-pre-start-game-roster-packets-before-start-game.md

### Requirement: Exact protocol version
The server SHALL accept only `PROTOCOL_VERSION` in both `RequestNetworkSettings` and `Login`. A mismatch MUST answer `PlayStatus` (`LoginFailedClientOld` when the client is older, `LoginFailedServerOld` otherwise) and fail with `ProtocolError::IncompatibleProtocol`, for which no further `Disconnect` is sent. The client MUST report a `PlayStatus` refusal as `JolyneError::LoginRefused`.

#### Scenario: Old client
- **WHEN** a client requests network settings with protocol 1
- **THEN** the server queues `PlayStatus(LoginFailedClientOld)`, fails with `IncompatibleProtocol { client_protocol: 1, .. }` whose `disconnect_reason()` is `None`
- **AND** the client receiving that status fails with `LoginRefused(LoginfailedClientold)`

Evidence: crates/jolyne/src/proto/server.rs (`ServerHandshake::protocol_mismatch`, `ServerLogin::on_packet`), crates/jolyne/src/proto/client.rs (`ClientSettings::on_packet`), crates/jolyne/src/proto/tests.rs (`protocol_mismatch_answers_play_status_and_client_reports_refusal`)

### Requirement: Compression negotiation
The server SHALL send `NetworkSettings` carrying the configured algorithm and threshold uncompressed, then enable that compression for both directions. Once negotiated, every batch MUST carry an algorithm byte, a payload shorter than the threshold MUST use the `0xFF` (none) marker, and inbound batches MUST be accepted with any known marker. The client MUST adopt the advertised algorithm and threshold.

#### Scenario: Defaults
- **WHEN** `CompressionConfig::default()` is used
- **THEN** the algorithm is raw DEFLATE (marker `0x00`, advertised as `Zlib`) at level 7 with threshold 1; Snappy is marker `0x01`

#### Scenario: Levels
- **WHEN** a deflate level above 9 is configured
- **THEN** it is clamped to 9, and level 0 or algorithm `None` sends every batch with the `0xFF` marker

#### Scenario: Bad markers
- **WHEN** a negotiated connection receives an empty batch body or an unknown algorithm byte
- **THEN** decoding fails with `ProtocolError::EmptyCompressedBatch` or `ProtocolError::UnknownCompression`

#### Scenario: Unknown advertised algorithm
- **WHEN** a client receives `NetworkSettings` with an algorithm it does not know
- **THEN** it fails with `ProtocolError::UnsupportedNetworkCompression`

#### Scenario: Compression stays on in play
- **WHEN** a join completes under Deflate, Snappy or None with threshold 0
- **THEN** play packets travel compressed and encrypted in both directions

Evidence: crates/jolyne/src/config.rs (`CompressionConfig`), crates/jolyne/src/proto/batch.rs (`BatchCodec`, `threshold_selects_marker_and_roundtrips_all_algorithms`, `rejects_wrong_marker_empty_payload_and_unknown_algorithm`), crates/jolyne/src/proto/compression.rs (`engine_follows_the_negotiated_algorithm_and_level`), crates/jolyne/src/proto/conn.rs (`enabling_compression_and_encryption_seals_pending_in_the_old_format`), crates/jolyne/src/proto/tests.rs (`compression_and_encryption_are_active_in_play`)

### Requirement: Batch framing and inbound limits
A batch SHALL be `[0xFE when RakNet][algorithm byte once negotiated][(varuint length, header varuint, body)*]`, where the header holds a ten-bit packet ID and two two-bit subclient IDs. Inbound batches MUST be bounded by `BatchLimits` and a violation MUST be a terminal error for the connection.

#### Scenario: Missing RakNet marker
- **WHEN** a `Framing::RakNet` connection receives a message whose first byte is not `0xFE`
- **THEN** it fails with `ProtocolError::InvalidBatchId` and counts a decode error

#### Scenario: Default limits
- **WHEN** a batch decompresses to more than 16 MiB (16,777,216 bytes) or holds more than 1,600 packets
- **THEN** decoding fails with `ProtocolError::BatchTooLarge` or `ProtocolError::TooManyPackets`, whether the payload was compressed, sent with the none marker, or sent before negotiation

#### Scenario: Truncated or overlong frames
- **WHEN** a frame's declared length exceeds the remaining bytes
- **THEN** decoding fails with `ProtocolError::PacketLength`

#### Scenario: Unprefixed framing
- **WHEN** a `Framing::Unprefixed` connection negotiates Snappy
- **THEN** its batches start with the algorithm byte and no `0xFE` marker, and round-trip

Evidence: crates/jolyne/src/proto/batch.rs (`BatchLimits`, `size_guard_covers_compressed_none_and_plain_payloads`, `typed_decode_enforces_packet_cap`, `frame_callback_rejects_truncated_lengths_and_packet_floods`), crates/jolyne/src/proto/raw.rs (`DEFAULT_MAX_PACKETS_PER_BATCH`, `parses_ids_subclients_and_multibyte_ids`, `rejects_truncated_and_overlong_frames`, `packet_cap_is_exact`, `decode_rejects_trailing_body_bytes_in_both_paths`), crates/jolyne/src/proto/conn.rs (`raknet_frames_must_carry_the_batch_marker`, `unprefixed_framing_roundtrips_with_compression`)

### Requirement: Unknown and custom packet IDs
Packet IDs that are neither valentine packets nor registered custom packets SHALL follow `ConnectionOptions::unknown_packets`: `Reject` (the server default) MUST fail the batch with `ProtocolError::UnknownPacket`, `Skip` MUST drop and count them, and `Passthrough` (the client default) MUST deliver them byte-identical with `name() == None`. A `PacketRegistry` MUST refuse IDs above `0x3FF` or already used by valentine.

#### Scenario: Server rejects, client passes through
- **WHEN** a packet with ID 1000 arrives
- **THEN** a default server connection fails with `UnknownPacket(1000)` (disconnect reason `Unknownpacket`), and a default client connection yields it with an identical frame and counts it in `metrics().unknown_packets`

#### Scenario: Registered custom packet
- **WHEN** ID 1000 is registered as a custom packet on a connection with the `Reject` policy
- **THEN** it is delivered, `decode_custom` returns the value and `decode` fails with `JolyneError::NonValentinePacket`

Evidence: crates/jolyne/src/proto/custom.rs (`UnknownPacketPolicy`, `PacketRegistry`, `registry_rejects_valentine_and_out_of_range_ids`), crates/jolyne/src/config.rs (`ClientConfig::default`), crates/jolyne/src/proto/raw.rs (`unknown_policy_rejects_skips_or_passes_through`, `custom_packets_must_use_free_ten_bit_ids`), crates/jolyne/src/proto/tests.rs (`unknown_and_custom_packets_follow_the_configured_policy`)

### Requirement: Outbound batching and backpressure
Outbound packets SHALL be framed into a pending batch sealed by the `FlushMode` (`Immediate` default, `Manual`, `Interval`) and always sealed at `max_packets_per_outbound_batch` or `max_pending_bytes`. Sealed bytes awaiting the transport MUST stay within `max_queued_bytes`; a batch that would exceed it MUST be dropped with `ProtocolError::WriteBufferFull` without advancing the cipher. A failed encode MUST leave earlier packets intact.

#### Scenario: Default bounds
- **WHEN** `ConnectionOptions::default()` is used
- **THEN** batches auto-seal at 812 packets or 4 MiB pending, and sealed output is capped at 64 MiB

#### Scenario: Interval flush
- **WHEN** packets are queued under `FlushMode::Interval(50 ms)`
- **THEN** they are sealed together once 50 ms have passed since the first one, and not before

#### Scenario: Refused batch keeps the cipher in sync
- **WHEN** an encrypted connection refuses a batch with `WriteBufferFull`
- **THEN** batches sent afterwards still decrypt and verify at the peer

#### Scenario: Join groups leave as single batches
- **WHEN** the server sends the `StartGame` group, or answers `RequestChunkRadius`, under `FlushMode::Immediate`
- **THEN** the group, and `ChunkRadiusUpdated` with `PlayStatus(PlayerSpawn)`, each leave as one batch

Evidence: crates/jolyne/src/config.rs (`ConnectionOptions`, `FlushMode`), crates/jolyne/src/proto/conn.rs (`flush_modes_immediate_manual_interval_and_auto_seal_limits`, `refused_batch_leaves_the_cipher_in_sync`, `failed_encode_keeps_the_pending_batch_intact`, `switching_to_interval_arms_the_timer_for_pending_packets`, `send_batch_splits_at_the_limit_with_the_requested_reliability`), crates/jolyne/src/proto/tests.rs (`start_game_and_spawn_answers_leave_as_single_batches`), crates/jolyne/src/stream/tests.rs (`manual_and_interval_flush_batch_packets`, `sends_wait_for_transport_capacity`)

### Requirement: Encryption handshake
When `encryption_enabled`, the server SHALL send `ServerToClientHandshake` in plaintext (an ES384 JWT with its P-384 key in `x5u` and a 16-byte random salt), then switch both directions to AES-256-CTR keyed by `SHA256(salt || ECDH(P-384) x)` with the key's first 12 bytes as IV. Each encrypted batch MUST carry an 8-byte checksum, and a checksum failure MUST invalidate the cipher permanently. Unreliable batches MUST be refused while encrypted.

#### Scenario: Key derivation vector
- **WHEN** the key is derived from private keys `0x01`×48 and `0x02`×48 and salt `00..0f`
- **THEN** it equals the independent Node/OpenSSL vector

#### Scenario: Client pins ES384
- **WHEN** the client receives a handshake token whose `alg` is not ES384, whose signature does not verify against its own `x5u`, or a second `ServerToClientHandshake`
- **THEN** it fails with `ProtocolError::InvalidHandshake`

#### Scenario: Tampered batch
- **WHEN** an encrypted batch is altered in transit
- **THEN** the receiver fails with `ChecksumMismatch`, zeroizes the plaintext, and every later batch fails with `CipherInvalidated`

#### Scenario: Client can require encryption
- **WHEN** `ClientConfig::require_encryption` is set and the server sends `PlayStatus(LoginSuccess)` without a handshake
- **THEN** the client fails with `InvalidHandshake`; with the default `false` it continues

Evidence: crates/jolyne/src/proto/handshake.rs (`derive_key_matches_an_independent_vector`, `token_roundtrip_derives_matching_keys`, `rejects_non_es384_and_tampered_tokens`), crates/jolyne/src/proto/crypto.rs (`BatchCipher`, `known_vectors_preserve_continuous_stream_and_batch_counter`, `tampering_zeroizes_plaintext_and_invalidates_cipher_permanently`, `exhausted_keystream_invalidates_instead_of_panicking`), crates/jolyne/src/proto/conn.rs (`checksum_failure_is_terminal_for_the_connection`, `unreliable_batches_are_refused_once_encrypted`), crates/jolyne/src/proto/tests.rs (`client_can_require_encryption`)

### Requirement: Bounded deferral of early packets
A join phase that receives a packet belonging to a later phase SHALL keep it, compacted and in arrival order, and replay it ahead of unread packets into the next owner or the play session. The buffer MUST be bounded by `max_deferred_packets` and `max_deferred_bytes`, and overflowing it MUST fail the phase with `ProtocolError::DeferredOverflow`.

#### Scenario: Early packets reach play in order
- **WHEN** the client sends `ClientCacheStatus` during login and the server sends definition packets and `ChunkRadiusUpdated` during spawn
- **THEN** the server session's first packet is `ClientCacheStatus`, and the client session reads `BiomeDefinitionList`, `AvailableActorIdentifiers`, `CreativeContent`, `ChunkRadiusUpdated` in that order

#### Scenario: Overflow
- **WHEN** more packets or bytes are deferred than configured (defaults 4,096 packets and 16 MiB)
- **THEN** the phase fails with `DeferredOverflow`

Evidence: crates/jolyne/src/proto/phase.rs (`Deferred`), crates/jolyne/src/config.rs (`ConnectionOptions::default`), crates/jolyne/src/proto/tests.rs (`packets_before_their_phase_are_replayed_into_play_in_order`, `deferring_more_than_the_configured_bound_fails_the_join`), crates/jolyne/src/proto/raw.rs (`compact_copy_detaches_from_batch_allocation`)

### Requirement: Mandatory phase deadlines
Every join phase SHALL have an absolute deadline, checked first in `advance`, that also bounds every write and wait of the async step. Reaching the deadline MUST fail with `ProtocolError::Timeout(HandshakePhase)`, and huge configured timeouts MUST be capped instead of overflowing `Instant`.

#### Scenario: Default deadlines
- **WHEN** default configs are used
- **THEN** the server's `login_timeout` (30 s, from accept through `ClientToServerHandshake`, including login verification), `resource_pack_timeout` (5 min) and `spawn_timeout` (60 s, from `StartGame`) and the client's `join_timeout` (120 s, every phase) apply

#### Scenario: Deadline is exclusive
- **WHEN** `login_timeout` is 3 s
- **THEN** `advance` at 2.999 s waits and `advance` at exactly 3 s fails

#### Scenario: Duration::MAX
- **WHEN** every timeout, and an interval flush, is `Duration::MAX`
- **THEN** each deadline lies more than 50 years ahead and nothing panics

#### Scenario: A stalled peer cannot hold a step
- **WHEN** the peer stops answering or reading during an async step
- **THEN** the step fails at its deadline

Evidence: crates/jolyne/src/proto/phase.rs (`deadline_after`), crates/jolyne/src/config.rs (`BedrockListenerConfig::default`, `ClientConfig::default`), crates/jolyne/src/stream/server/mod.rs (`authenticate`), crates/jolyne/src/proto/tests.rs (`each_stage_has_a_mandatory_deadline`, `deadline_elapses_exactly_at_login_timeout`, `huge_configured_timeouts_mean_far_deadlines_not_overflow`, `resource_pack_timeouts_and_failures_have_their_own_disconnect_reasons`), crates/jolyne/src/stream/tests.rs (`advancing_is_bounded_by_the_step_deadline`)

### Requirement: Spawn completion
After `StartGame` the server SHALL answer the first `RequestChunkRadius` (radius at least 1, else `ProtocolError::InvalidChunkRadius`) with `ChunkRadiusUpdated` carrying `WorldJoinParams::chunk_radius` when set, and complete at a `SetLocalPlayerAsInitialized` whose runtime ID equals the `StartGame` one, else `ProtocolError::RuntimeIdMismatch`. A streamed spawn MUST hand over the session at the radius request and enforce both checks on the play receive path.

#### Scenario: Server override
- **WHEN** the client requests radius 16 and the join params set radius 4
- **THEN** both sessions report chunk radius 4; a request of 0 fails with `InvalidChunkRadius`

#### Scenario: Plain spawn announces no terrain
- **WHEN** `start_game` is used
- **THEN** the radius answer is `ChunkRadiusUpdated` plus `PlayStatus(PlayerSpawn)` with no `NetworkChunkPublisherUpdate`

#### Scenario: Streamed spawn
- **WHEN** `start_game_streamed` is used
- **THEN** the radius answer is `ChunkRadiusUpdated` only, the session reports `is_initialized() == false`, and play receives fail with `RuntimeIdMismatch` or `Timeout(Spawn)` until a matching `SetLocalPlayerAsInitialized` arrives

Evidence: crates/jolyne/src/proto/server.rs (`ServerSpawn`), crates/jolyne/src/proto/session.rs (`Session::next_packet`), crates/jolyne/src/proto/tests.rs (`chunk_radius_requires_one_and_honours_the_server_override`, `runtime_id_mismatch_is_rejected`, `spawn_does_not_announce_terrain_it_does_not_send`, `streamed_spawn_lets_the_application_deliver_the_view_first`, `streamed_spawn_keeps_the_spawn_deadline_and_runtime_id_check`), crates/jolyne/src/stream/tests.rs (`streamed_spawn_completes_on_the_play_receive_path`, `streamed_spawn_times_out_on_the_play_receive_path`)

### Requirement: Refusals and disconnects
A failed step SHALL keep its connection in `Failed`; `Failed::disconnect()` MUST send a `Disconnect` whose reason is mapped from the error and whose message is one fixed sentence per reason, never the error text. Refusals MUST be answered by the phase itself: `PlayStatus(LoginFailedServerFull)` when `max_players` is reached and `Disconnect{Kicked}` with the hook's message when admission refuses. An inbound `Disconnect` MUST surface once as `JolyneError::Disconnected`, then `ConnectionClosed`.

#### Scenario: Fixed messages
- **WHEN** a join fails with an auth error containing token text, or a decompression error containing a hex preview
- **THEN** the peer receives `Notauthenticated` "Authentication failed." or `Badpacket` "Received an invalid packet.", without the error text

#### Scenario: Server full and admission refusal
- **WHEN** the listener is full, or the admission hook returns `Err("whitelist")`
- **THEN** the client fails with `LoginRefused(LoginfailedServerfullsubclient)`, or with `Disconnected` carrying reason `Kicked` and message "whitelist"

#### Scenario: Peer disconnect
- **WHEN** the peer sends `Disconnect{Kicked, "bye"}`
- **THEN** `next_packet` returns `Disconnected` with that reason, message and raw packet, and the next call returns `ConnectionClosed`

#### Scenario: Bounded close
- **WHEN** `close` or `disconnect` runs while the peer has stopped reading
- **THEN** it returns within the 5 s close timeout

Evidence: crates/jolyne/src/error.rs (`disconnect_reason`, `disconnect_message`), crates/jolyne/src/stream/failed.rs (`Failed::disconnect`), crates/jolyne/src/stream/driver.rs (`CLOSE_TIMEOUT`), crates/jolyne/src/stream/slots.rs (`slots_cap_and_release`), crates/jolyne/src/proto/tests.rs (`failure_disconnects_never_send_internal_error_text`, `server_full_and_admission_refusals_reach_the_client_typed`), crates/jolyne/src/proto/conn.rs (`inbound_disconnect_is_a_typed_error_then_closed`), crates/jolyne/src/stream/tests.rs (`join_echo_and_typed_disconnect`, `close_is_bounded_when_the_peer_stops_reading`, `close_and_drop_flush_buffered_packets`), crates/jolyne/tests/raknet_listener.rs (`listener_advertises_status_and_enforces_max_players`)

### Requirement: Transport independence
The join and play logic SHALL run over any `stream::Transport`, whose `FRAMING` selects `Framing::RakNet` or `Framing::Unprefixed`; RakNet (feature `raknet`) and NetherNet (feature `nethernet`) adapters MUST be provided behind their features. Sends MUST wait for `poll_send_ready`, reliable transmits MUST map to RakNet `ReliableOrdered`, and a RakNet disconnection notification MUST surface as `ConnectionClosed`, not a transport error.

#### Scenario: Join over NetherNet
- **WHEN** a client and listener join over a loopback NetherNet connection with Bedrock encryption off
- **THEN** the join completes and play packets are exchanged

#### Scenario: Join over RakNet
- **WHEN** the example client joins the example server over RakNet loopback
- **THEN** the join completes

#### Scenario: Normal peer close
- **WHEN** a RakNet peer closes with `ClosedByRemotePeer`
- **THEN** the receive reports `JolyneError::ConnectionClosed`; a timeout stays an error

Evidence: crates/jolyne/src/stream/transport/mod.rs (`Transport`), crates/jolyne/src/stream/transport/raknet.rs (`only_the_peers_disconnection_notification_is_a_normal_close`), crates/jolyne/src/stream/transport/nethernet.rs, crates/jolyne/Cargo.toml (features), crates/jolyne/tests/nethernet_loopback.rs (`full_join_over_nethernet_loopback`), crates/jolyne/tests/examples_smoke.rs (`example_client_joins_example_server`), crates/jolyne/src/stream/tests.rs (`a_normal_peer_close_is_connection_closed_not_a_transport_error`, `cancelled_receive_keeps_queued_packets`)
Verified by: `.github/workflows/test.yml` (clippy for each `client`/`server` × `raknet`/`nethernet`/`discovery` feature set, `cargo test -p jolyne --all-features`)
