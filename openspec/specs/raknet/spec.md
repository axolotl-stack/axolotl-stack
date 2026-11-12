# raknet Specification

## Purpose
`crates/tokio-raknet` is the asynchronous RakNet transport used by the Bedrock stack: a `RaknetListener` that accepts sessions and a `RaknetStream` that is both the client dial result and the accepted server-side handle. It owns the offline/online handshake, MTU negotiation, reliability, ordering, splitting/reassembly, ACK/NAK feedback, keepalive, timeouts and disconnect. It carries opaque payload bytes; Bedrock game-packet framing, compression, encryption and login live in higher layers (`jolyne`, `valentine`). Per-behaviour wire evidence and reference comparisons are catalogued in `crates/tokio-raknet/PARITY.md`; this spec states the contract that catalogue's tests protect.

## Requirements

### Requirement: Listener and stream API shape
The transport SHALL expose `RaknetListener` (`bind`, `bind_with_config`, `accept`, `poll_accept`, `local_addr`, `guid`, `set_advertisement`, `get_advertisement`) and `RaknetStream` (`connect`, `connect_with_config`, `recv_message`, `poll_recv_message`, `send_encoded`, `local_addr`, `peer_addr`, `rtt`), both re-exported at the crate root. `RaknetStream` MUST implement `Stream<Item = Result<Bytes, RaknetError>>` and `Sink<Message>`.

#### Scenario: Client and server exchange messages
- **WHEN** `RaknetStream::connect` dials a `RaknetListener` bound on loopback and both sides send a message
- **THEN** `accept` returns a `RaknetStream` whose `peer_addr` is the client address
- **AND** each side's `Stream` yields the other side's message bytes with the ID byte first

#### Scenario: Message ID and payload are exposed separately
- **WHEN** the peer sends a message whose first byte is `0xFE`
- **THEN** `recv_message` returns a `ReceivedMessage` with `id` `0xFE`, the remaining bytes as `payload`, and its `reliability` and `channel`

#### Scenario: RTT is unknown until acknowledged
- **WHEN** a stream has not yet received an acknowledgement sample
- **THEN** `rtt()` returns `None`, and after acknowledgements it returns `Some`

Evidence: crates/tokio-raknet/src/lib.rs, crates/tokio-raknet/src/transport/stream.rs, crates/tokio-raknet/src/transport/listener.rs, crates/tokio-raknet/tests/integration_handshake.rs (`test_basic_handshake_and_exchange`, `test_recv_message_exposes_id_and_payload_without_repacking`, `stream_rtt_is_published_after_acknowledgements`, `test_stream_addresses`)

### Requirement: Message delivery options and defaults
`Message::new` SHALL default to `Reliability::ReliableOrdered`, channel 0 and `RakPriority::Normal`, and the `reliability`, `channel` and `priority` builders MUST override exactly those fields. Conversions from `Vec<u8>`, `&'static [u8]`, `&str` and `String` MUST use the `Message::new` defaults, while conversion from `Bytes` MUST use `Reliability::UnreliableSequenced`. Sending an empty buffer MUST be a no-op returning `Ok`.

#### Scenario: Builder overrides delivery options
- **WHEN** a caller builds `Message::new(bytes).reliability(Reliability::Reliable).channel(1).priority(RakPriority::High)`
- **THEN** the message carries exactly those values and the original buffer

#### Scenario: String sends are reliable ordered
- **WHEN** `send_encoded("text")` is called
- **THEN** the message is queued as `ReliableOrdered` on channel 0

#### Scenario: Bytes sends are unreliable sequenced
- **WHEN** `send_encoded(bytes)` is called with a `Bytes` value
- **THEN** the message is queued as `UnreliableSequenced` on channel 0

Evidence: crates/tokio-raknet/src/transport/mod.rs (`Message` and its `From` impls), crates/tokio-raknet/src/transport/stream.rs (`start_send`), crates/tokio-raknet/tests/integration_handshake.rs (`test_message_builder_integration`)

### Requirement: Configuration is validated before any socket is bound
`bind_with_config` and `connect_with_config` SHALL validate their configuration before binding a socket and MUST reject out-of-range values with a typed `ConfigError` naming the field and requirement, never clamping silently or panicking. The listener MUST return `io::ErrorKind::InvalidInput` wrapping the `ConfigError`; the client MUST return that `io::Error` inside `RaknetError::Io`.

#### Scenario: Rejected ranges
- **WHEN** a config has an MTU (`max_mtu` / `mtu`) outside `400..=1492`, a zero or non-representable duration, zero `max_connections`, `max_pending_connections` or `max_queued_reliable_bytes`, or a socket buffer size of `Some(0)`
- **THEN** `validate()` returns `Err(ConfigError)`
- **AND** the same holds for `max_ordering_channels` outside `1..=256`, `ack_queue_capacity` or `reliable_window` outside `1..=2048`, `max_split_parts` outside `1..=512`, `max_concurrent_splits` outside `1..=16`, `max_incoming_ack_queue` outside `1..=8192`, `session_stale` above `session_timeout`, or an advertisement over 32767 bytes

#### Scenario: Listener MTU below the minimum
- **WHEN** `bind_with_config` is called with `max_mtu: 1`
- **THEN** it returns an `io::Error` of kind `InvalidInput` whose inner error is a `ConfigError`, without binding a socket or panicking

#### Scenario: Oversized advertisement is rejected without side effects
- **WHEN** `set_advertisement` is called with more than 32767 bytes
- **THEN** it returns `Err(ConfigError)` and `get_advertisement` still returns the previous advertisement

Evidence: crates/tokio-raknet/src/transport/mod.rs (`validate_mtu`, `validate_session_limits`), crates/tokio-raknet/src/error/mod.rs (`ConfigError`), crates/tokio-raknet/src/transport/listener.rs (`invalid_listener_config_is_rejected`, `invalid_config_returns_typed_error_before_bind`), crates/tokio-raknet/src/transport/stream.rs (`invalid_stream_config_is_rejected`), crates/tokio-raknet/tests/integration_handshake.rs (`test_listener_set_advertisement`)

### Requirement: Transmission policy selection
The transport SHALL provide `TransmissionPolicy::Vanilla` (Mojang RakNet behaviour) and `TransmissionPolicy::Optimized` (tokio-raknet tuning), selectable per connection through `transmission_policy` on both configs. Wire format, ordering/reassembly correctness and the resource bounds in this spec MUST hold under both policies unless a requirement names a policy.

#### Scenario: Default policies
- **WHEN** `RaknetStreamConfig::default()` and `RaknetListenerConfig::default()` are constructed
- **THEN** the stream policy is `Vanilla` and the listener policy is `Optimized`, as is the low-level `Session` default

#### Scenario: Vanilla matches recorded captures
- **WHEN** `cargo test -p tokio-raknet --test capture_diff` replays the committed captures in `tests/captures` against a Vanilla listener and a Vanilla stream
- **THEN** the offline probe, connection-accept layout, fragment sizes, ACK delay, NAK/resend latency, ping cadence and timeout handling match within the test tolerances

Evidence: crates/tokio-raknet/src/transport/stream.rs, crates/tokio-raknet/src/transport/listener.rs, crates/tokio-raknet/tests/capture_diff.rs, crates/tokio-raknet/PARITY.md

### Requirement: Client MTU discovery and negotiated MTU
A connecting `RaknetStream` SHALL probe MTU sizes `[cap, 1200, 576]` (filtered by `RaknetStreamConfig::mtu`) 500 ms apart, accept a reply to any earlier probe, and tolerate up to `max_transient_errors` (default 10) transient receive errors. It MUST speak RakNet protocol version 11, and an incompatible-protocol reply MUST end discovery with `RaknetError::IncompatibleProtocolVersion`. `Optimized` MUST reject an invalid Reply1/Reply2 rather than clamp it.

#### Scenario: Vanilla probe ladder exhausts
- **WHEN** a Vanilla stream dials a peer that never answers
- **THEN** 13 Request1 probes (4, 4, then 5 per size) are sent 500 ms apart and the attempt fails at 6.5 s

#### Scenario: Incompatible protocol
- **WHEN** the server answers Request1 with an incompatible-protocol reply
- **THEN** the connect fails immediately with `RaknetError::IncompatibleProtocolVersion`

#### Scenario: Vanilla clamps the granted MTU
- **WHEN** a Vanilla stream receives a Reply2 MTU of 300 or 2000
- **THEN** the connection MTU is `max(576, clamp(mtu, 400, 1492))`, i.e. 576 or 1492

Evidence: crates/tokio-raknet/src/transport/handshake.rs (`probe_sizes_follow_go_mtu_sizes_for`, `vanilla_sends_thirteen_probes_then_fails_at_6500ms`, `vanilla_probing_continues_after_reply1_until_reply2`, `vanilla_reply2_mtu_is_clamped_like_mojang`, `incompatible_protocol_fails_discovery_immediately`, `optimized_reply2_validation_is_strict`, `transient_receive_errors_are_tolerated_up_to_the_limit`), crates/tokio-raknet/src/protocol/constants.rs

### Requirement: Listener offline handshake admission
Both listener policies SHALL refuse new sessions with `NoFreeIncomingConnections` at `max_connections`, enforce `max_pending_connections`, rate-limit pings and Request1 per IP (`ping_rate_limit_per_ip`, tracked IPs bounded by `ping_rate_limit_max_entries`), and drop a half-open session 10 s after creation regardless of retries. The `Optimized` listener MUST require address-bound cookies; the `Vanilla` listener MUST answer like the Bedrock dedicated server.

#### Scenario: Optimized Request2 without the address cookie
- **WHEN** an Optimized listener receives Request2 whose cookie was not issued to that address in the current or previous 2 s rotation, or whose client GUID is non-negative
- **THEN** no Reply2 is sent and the source IP is blocked for 10 s
- **AND** a Request2 MTU outside `400..=1492`, an incompatible protocol, or an unknown offline packet ID also block the IP for 10 s

#### Scenario: Optimized MTU grant
- **WHEN** an Optimized listener accepts Request2
- **THEN** the session MTU is `min(max_mtu, requested)` and Reply1 grants were clamped to `[400, max_mtu]`

#### Scenario: Vanilla listener answers like the dedicated server
- **WHEN** a Vanilla listener receives a 1492-byte probe and then Request2
- **THEN** Reply1 carries no cookie and grants `min(payload + 28, min(max_mtu, 1400))`, Reply2 echoes the requested MTU, and the session MTU is `max(576, clamp(mtu, 400, min(max_mtu, 1400)))`
- **AND** GUID/address conflicts get `AlreadyConnected` and a same-IP connection within 50 ms gets `IpRecentlyConnected` (loopback exempt)

#### Scenario: Half-open session frees its slot
- **WHEN** a peer completes Request2 against a listener with `max_connections: 1` and then goes silent for more than 10 s
- **THEN** the slot is freed and a new client can connect and be accepted

Evidence: crates/tokio-raknet/src/transport/listener/offline.rs (`request1_is_stateless_and_request2_requires_address_cookie`, `reply1_wire_per_policy`, `vanilla_reply1_mtu_is_payload_plus_28_capped_at_1400`, `vanilla_reply2_echoes_requested_mtu_and_session_mtu_is_clamped`, `vanilla_same_ip_within_50ms_is_ip_recently_connected`, `vanilla_guid_or_address_conflict_is_already_connected`), crates/tokio-raknet/src/transport/listener/rate_limiter.rs (`cookies_are_address_bound_and_expire_after_two_rotations`, `blocks_expire_and_are_shared_by_source_ip`, `test_rate_limiter_caps_tracked_ip_cardinality`), crates/tokio-raknet/tests/integration_handshake.rs (`test_half_open_request2_session_times_out_and_frees_slot`, `test_request2_rechecks_max_connections`)

### Requirement: Online handshake gates application data
The listener SHALL surface a connection through `accept` only after the client's `NewIncomingConnection` completes the online handshake, and user data received before that MUST NOT be delivered. A connection whose accept-queue slot is unavailable MUST be retried once capacity returns. Connection-accept and new-incoming-connection packets MUST carry up to 20 system addresses.

#### Scenario: Forged NewIncomingConnection
- **WHEN** a peer sends `NewIncomingConnection` without having sent `ConnectionRequest`
- **THEN** `accept` does not return a stream for that peer

#### Scenario: Pre-handshake user data
- **WHEN** a session receives a user message before the handshake completes
- **THEN** the message is rejected and the listener keeps serving other peers

#### Scenario: Twenty system addresses
- **WHEN** a `ConnectionRequestAccepted` with 20 system addresses and two timestamps is decoded
- **THEN** all 20 addresses and both timestamps are preserved

Evidence: crates/tokio-raknet/tests/integration_handshake.rs (`test_listener_waits_for_new_incoming_connection_before_accept`, `test_forged_new_incoming_connection_does_not_reach_accept_queue`, `test_accept_queue_full_connection_is_retried_after_capacity_returns`, `test_pre_accept_user_data_flood_does_not_block_listener`), crates/tokio-raknet/src/session/manager.rs (`rejects_user_data_before_handshake`), crates/tokio-raknet/src/protocol/packet/open_connection.rs (`connection_request_accepted_with_twenty_addresses_keeps_timestamps`)

### Requirement: Reliability and ordering channels
The transport SHALL support every RakNet reliability, deliver ordered messages in order per channel, assign sequence indices to sequenced messages, and drop duplicate reliable messages. The default ordering-channel count MUST be 16, and a channel at or above the configured count MUST surface an error to the application rather than be remapped. Retransmissions MUST use fresh datagram sequence numbers while preserving reliable and ordering indices.

#### Scenario: Out-of-order arrival is reordered
- **WHEN** ordered messages with indices 1, a duplicate 1, and 2 arrive before index 0 on the same channel
- **THEN** they are delivered as 0, 1, 2 with index 1 delivered once

#### Scenario: Invalid channel
- **WHEN** an application on either side sends on a channel at or above `max_ordering_channels`
- **THEN** the application receives an error rather than the message being sent on another channel

#### Scenario: Loss recovery
- **WHEN** a reliable ordered datagram is lost and later retransmitted
- **THEN** ordered delivery resumes with every message delivered once

Evidence: crates/tokio-raknet/src/session/ordering_channels.rs (`out_of_order_buffering`, `buffered_duplicates_do_not_block_delivery`, `invalid_channel_returns_none`), crates/tokio-raknet/src/session/reliable_tracker.rs (`drops_duplicates`), crates/tokio-raknet/src/session/outbound.rs (`sequenced_packets_get_sequence_index`), crates/tokio-raknet/src/session/mod.rs (`retransmitted_loss_recovers_ordered_delivery`), crates/tokio-raknet/tests/integration_handshake.rs (`test_client_invalid_channel_queue_error_reaches_app`, `test_server_invalid_channel_queue_error_reaches_app`)

### Requirement: Bounded receive window
Receive state SHALL be bounded by a 2048-index window that is checked before any state is mutated or any gap is scanned, so a peer-chosen sequence gap never causes work proportional to its size. The reliable-message window (`reliable_window`, default and maximum 2048) and each ordering channel MUST likewise refuse to buffer indices beyond the window. Duplicate datagrams MUST NOT allocate window entries.

#### Scenario: Window boundary
- **WHEN** an Optimized datagram window at 0 receives sequence 2048
- **THEN** it returns `DecodeError::ReceiveWindowExceeded` with its state unchanged
- **AND** receiving 2047 afterwards is accepted and reports 0..=2046 as missing

#### Scenario: Hostile datagram gap
- **WHEN** an Optimized session receives datagram sequence `1 << 23` as its first datagram
- **THEN** the window does not advance, holds no entries, and the next tick emits no ACK/NAK feedback

#### Scenario: Hostile ordering index
- **WHEN** an ordered message arrives with ordering index 4096 on a fresh channel
- **THEN** nothing is buffered for that channel

#### Scenario: Vanilla datagram gap rule
- **WHEN** a Vanilla session receives a datagram skipping more than 50000 sequence numbers
- **THEN** the datagram is dropped unacknowledged, and smaller gaps NAK at most 1000 numbers each

Evidence: crates/tokio-raknet/src/session/receive_window.rs (`RECEIVE_WINDOW_SIZE`, `receive_window_boundary_is_checked_before_state_mutation`), crates/tokio-raknet/src/session/mod.rs (`hostile_datagram_gap_does_not_advance_or_queue_feedback`), crates/tokio-raknet/src/session/ordering_channels.rs (`hostile_ordering_index_does_not_buffer`), crates/tokio-raknet/src/session/reliable_tracker.rs (`rejects_too_far_ahead`), crates/tokio-raknet/src/session/policy.rs (`VANILLA_MAX_SKIPPED_DATAGRAMS`, `VANILLA_MAX_NAKS_PER_GAP`), crates/tokio-raknet/src/transport/stream.rs (`vanilla_client_accepts_gaps_up_to_50000_datagrams`)

### Requirement: Bounded ACK/NAK and outgoing control state
The transport SHALL accept an inbound ACK/NAK payload only while it expands to at most 8193 sequences and MUST reject wrapped ranges, unknown record tags and truncated records. Outgoing ACK/NAK datagrams MUST fit the MTU and cover at most 8192 sequences. Inbound ACK/NAK ranges MUST be capped at `max_incoming_ack_queue`, outgoing feedback at `ack_queue_capacity`, and peer-triggered responses at 2048 deferred packets per session.

#### Scenario: Go range boundary
- **WHEN** an ACK carries one range expanding to exactly 8193 sequences
- **THEN** it is accepted, and a payload exceeding that total is rejected

#### Scenario: Malformed ACK records
- **WHEN** an ACK record has an unknown tag, a wrapping range, or a truncated record count
- **THEN** decoding fails with an error

#### Scenario: Peer-triggered responses cannot grow the queue
- **WHEN** 2048 immediate unreliable pongs are already queued in a session
- **THEN** queuing one more fails instead of growing the queue

Evidence: crates/tokio-raknet/src/protocol/ack.rs (`accepts_go_range_boundary_of_8193_sequences`, `rejects_payload_with_too_many_total_sequences`, `rejects_wrapping_inbound_range`, `rejects_unknown_record_type`, `rejects_truncated_record_count`), crates/tokio-raknet/src/session/ack_queue.rs (`cannot_emit_feedback_beyond_mtu`), crates/tokio-raknet/src/session/mod.rs (`incoming_ack_queue_respects_limit`, `incoming_nack_queue_respects_limit`, `peer_triggered_responses_cannot_grow_outgoing_queue`), crates/tokio-raknet/src/protocol/constants.rs (`MAX_ACK_SEQUENCES`, `MAX_INBOUND_ACK_SEQUENCES`)

### Requirement: Splitting and bounded reassembly
Messages larger than one datagram SHALL be split and reassembled transparently, and a reassembled message MUST be delivered when its final fragment arrives without waiting for an ACK deadline or maintenance tick. Reassembly MUST be bounded by `max_split_parts` (default and maximum 512) and `max_concurrent_splits` (default and maximum 16), validated before allocating, and incomplete splits MUST expire after `split_timeout` (default 30 s). Split fragments MUST be sent reliably.

#### Scenario: Hostile split count
- **WHEN** a fragment declares 513 parts
- **THEN** reassembly fails with `DecodeError::SplitTooLarge` and no entry is allocated

#### Scenario: Too many concurrent splits
- **WHEN** a new split ID arrives while the concurrent-split limit is already in use
- **THEN** it fails with `DecodeError::SplitBufferFull`

#### Scenario: Invalid split metadata
- **WHEN** a fragment has a zero count, an out-of-range index, or reuses a live split ID on another ordering channel
- **THEN** it is rejected without consuming the split ID

#### Scenario: Expired split unblocks ordering
- **WHEN** an ordered split stays incomplete past `split_timeout` with a later ordered message buffered behind it
- **THEN** the next tick drops the split and delivers the later message

#### Scenario: Vanilla fragment size
- **WHEN** a Vanilla session splits a message
- **THEN** each fragment payload is `MTU - 60` bytes and unreliable reliabilities are upgraded to reliable ones

Evidence: crates/tokio-raknet/src/session/split_assembler.rs (`hostile_split_count_rejected_before_allocating`, `rejects_too_many_parts`, `rejects_when_buffer_full`, `invalid_index_does_not_consume_split_id`, `split_id_cannot_mix_ordering_channels`, `out_of_order_parts_ignore_duplicates_and_rejected_fragments`), crates/tokio-raknet/src/session/tick.rs (`split_timeout_releases_buffered_ordered_packet`), crates/tokio-raknet/src/session/manager.rs (`split_delivery_and_ack_do_not_wait_for_maintenance`), crates/tokio-raknet/src/session/vanilla.rs (`split_fragments_are_mtu_minus_60_and_reliable`), crates/tokio-raknet/src/protocol/encapsulated_packet.rs (`rejects_invalid_split_metadata`)

### Requirement: Bounded send and receive queues
A listener session SHALL disconnect with `DisconnectReason::QueueTooLong` when its queued reliable data exceeds `max_queued_reliable_bytes` (default 1 MiB, 4 MiB in `high_performance()`); `Optimized` MUST count unsent bytes and `Vanilla` MUST count reliable bytes until ACKed. `Vanilla` MUST keep at most 512 reliable messages in flight. A client stream MUST buffer up to 4096 unread received messages.

#### Scenario: Client receive queue capacity
- **WHEN** the server sends 4096 messages that the client application does not read
- **THEN** the connection remains open and all 4096 are readable

#### Scenario: Client receive queue overflow
- **WHEN** the server sends 4097 messages that the client does not read
- **THEN** the client disconnects and the server-side stream observes a disconnect or close within 4 s

#### Scenario: Vanilla unACKed bytes count toward the limit
- **WHEN** a Vanilla session's sent-but-unacknowledged reliable bytes exceed `max_queued_reliable_bytes`
- **THEN** the session disconnects with `QueueTooLong`

Evidence: crates/tokio-raknet/tests/integration_handshake.rs (`client_buffers_4096_unread_messages`, `full_client_receive_queue_notifies_server_of_disconnect`), crates/tokio-raknet/src/session/manager/tick.rs (`vanilla_queue_limit_counts_unacked_reliable_bytes`), crates/tokio-raknet/src/session/vanilla.rs (`new_sends_stop_at_512_messages_in_flight`), crates/tokio-raknet/src/transport/listener.rs (`high_performance`)

### Requirement: Connection and inactivity timeouts
`connect_with_config` SHALL bound the whole dial by `connection_timeout` (default 10 s) and fail with `RaknetError::Io` of kind `TimedOut` when it expires, and sessions still in the online handshake MUST close 10 s after creation. `Optimized` sessions MUST time out when nothing is received for `session_timeout` (default 5 s) plus twice the measured RTT, and outgoing traffic MUST NOT refresh receive activity.

#### Scenario: Dial against an inert peer
- **WHEN** a stream dials a UDP socket that never replies, with `connection_timeout` of 150 ms
- **THEN** connect fails in under 1 s with `RaknetError::Io` of kind `TimedOut`

#### Scenario: Keepalives do not keep a dead peer alive
- **WHEN** an Optimized session only sends keepalives and receives nothing past its timeout
- **THEN** the session times out

#### Scenario: Vanilla keepalive and silent drop
- **WHEN** a Vanilla session is connected
- **THEN** it sends two unreliable pings at connect and one every 5 s, and a reliable ping after 5 s with nothing unacknowledged
- **AND** if it has unacknowledged data and receives no datagram for 10 s it closes without sending a disconnect notification

Evidence: crates/tokio-raknet/tests/integration_handshake.rs (`test_stream_connect_timeout_is_enforced`), crates/tokio-raknet/src/session/manager.rs (`outgoing_keepalives_do_not_refresh_receive_timeout`), crates/tokio-raknet/src/session/manager/tick.rs (`vanilla_handshake_times_out_after_ten_seconds`, `vanilla_dead_connection_closes_silently_after_ten_seconds`, `vanilla_reliable_keepalive_after_five_idle_seconds`, `vanilla_server_pings_twice_on_connect_then_every_five_seconds`), crates/tokio-raknet/tests/capture_diff.rs (`silent_client_is_dropped_after_ten_seconds_like_bds`)

### Requirement: Disconnect and malformed-peer handling
Closing or dropping a stream SHALL notify the peer with a disconnect notification, and a received notification MUST end the stream with `RaknetError::Disconnected` or `ConnectionClosed`. Connection-failure packets received after dialing MUST end the connection with a typed `RaknetError`. Malformed datagrams MUST be handled per policy: `Optimized` ends the session (and a listener blocks the source IP for 10 s), `Vanilla` drops the datagram and keeps the connection.

#### Scenario: Vanilla client drop
- **WHEN** a Vanilla client stream is dropped
- **THEN** it queues a bare `0x15` disconnect notification and closes once the server acknowledges it

#### Scenario: Remote disconnect under Vanilla
- **WHEN** a Vanilla session receives a disconnect notification
- **THEN** it ACKs the datagram, does not echo the notification, and closes

#### Scenario: Malformed datagram per policy
- **WHEN** a client receives a malformed datagram from the server
- **THEN** an Optimized client reports an error and stops, while a Vanilla client keeps the connection

#### Scenario: Failure packet after dialing
- **WHEN** a connected client receives a connection-failure packet
- **THEN** the stream yields a typed `RaknetError`

Evidence: crates/tokio-raknet/src/transport/stream.rs (`vanilla_client_drop_sends_disconnect_and_closes_when_acked`, `malformed_server_datagram_reports_error_and_stops_muxer`, `vanilla_client_survives_malformed_datagram`, `connection_failure_packet_is_reported`), crates/tokio-raknet/src/session/manager/tick.rs (`vanilla_close_sends_bare_disconnect_and_closes_when_acked`, `vanilla_remote_disconnect_is_acked_not_echoed`), crates/tokio-raknet/src/transport/listener/online.rs (`vanilla_listener_ignores_invalid_datagrams_and_unknown_offline_ids`)

### Requirement: Optimized close drains on the caller's clock
An `Optimized` session closed with `close` or `disconnect_now` SHALL send one Reliable disconnect notification and stay `Closing` while reliable sends are unacknowledged, for at most 5 s, or 1 s when nothing is outstanding. The drain MUST be measured from the `now` the caller passes (`close`, `disconnect_now`, `send_disconnect` and the queue-limit path all take it), never from the wall clock.

#### Scenario: Unacknowledged sends drain for at most 5 s
- **WHEN** an Optimized session with unacknowledged reliable data is closed at `now`
- **THEN** it sends one Reliable disconnect notification, is still `Closing` at `now + 4.9 s`, and is `Closed` at `now + 5 s`

#### Scenario: Nothing outstanding
- **WHEN** an Optimized session with everything acknowledged is closed
- **THEN** it is still `Closing` at 900 ms and `Closed` at 1 s

#### Scenario: Caller's clock ahead of the wall clock
- **WHEN** a session driven with instants one minute ahead of the wall clock calls `close` or `disconnect_now` with unacknowledged data
- **THEN** it is still draining 1.5 s later instead of closing on the first tick

Evidence: crates/tokio-raknet/src/session/manager/tick.rs (`close`, `disconnect_now`, `enforce_queue_limit`, tests `optimized_close_drains_unacknowledged_sends_for_at_most_five_seconds`, `optimized_close_with_nothing_unacknowledged_closes_after_one_second`, `optimized_close_measures_the_drain_from_the_callers_now`, `optimized_disconnect_now_measures_the_drain_from_the_callers_now`), crates/tokio-raknet/src/session/manager/control.rs (`send_disconnect`)
