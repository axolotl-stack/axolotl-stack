# nethernet Specification

## Purpose
`crates/tokio-nethernet` is the asynchronous NetherNet transport: Bedrock connections over WebRTC (ICE, DTLS, SCTP data channels) negotiated through a pluggable signaling layer. It plays the same role for NetherNet that `raknet` plays for RakNet: it owns negotiation, identity assertions, framing, reliability, resource bounds and close, and carries opaque payload bytes. Game-packet framing, compression, encryption and Login run on top in `jolyne` (see `session-handshake`). It ships three signaling implementations behind features: encrypted LAN discovery (`discovery`), go-nethernet HTTP endpoint signaling (`endpoint`) and the retained franchise WebSocket used for Xbox friend joins (`xbox-signaling`, used by `friend-gateway`). Behaviour compared against the real client, BDS 1.26.51 and go-nethernet is catalogued per row in `crates/tokio-nethernet/PARITY.md`; this spec states the contracts those rows' tests protect.

## Requirements

### Requirement: Listener, dialer and stream API
The crate SHALL expose `NetherNetListener` (`bind_with_signaling`, `new`, `accept`, `poll_accept`), `NetherNetDialer` (`connect_with_signaling`, `new`, `dial`) and `NetherNetStream` (`connect`, `connect_with_config`, `remote_identity`, `rtt`) at the crate root. `NetherNetStream` MUST implement `Stream<Item = Result<Message, NetherNetError>>` and `Sink<Message>`, where `Message` carries `buffer: Bytes` and a `reliable` flag. Signal routing MUST be installed before an offer is sent.

#### Scenario: Loopback exchange in both reliability modes
- **WHEN** a dialer and a listener on `TransportConfig::loopback_v4()` negotiate over in-memory signaling and each sends one message
- **THEN** `accept` yields a stream that receives the dialer's reliable message with `reliable == true`
- **AND** the dialer receives the listener's unreliable message with `reliable == false`

#### Scenario: Closed listener
- **WHEN** the listener's signaling closes or its accept side is dropped
- **THEN** `accept` returns `Err(NetherNetError::ConnectionClosed)` and the listener's tracked connections are closed

#### Scenario: Synchronous answers are not lost
- **WHEN** a signaling implementation produces the answer inside the call that sends the offer (HTTP endpoint)
- **THEN** the dialer still receives it, because `connect_with_signaling` takes the signal receiver before the dial actor starts

Evidence: crates/tokio-nethernet/src/lib.rs, crates/tokio-nethernet/src/listener.rs, crates/tokio-nethernet/src/dialer.rs, crates/tokio-nethernet/src/stream.rs, crates/tokio-nethernet/tests/integration_test.rs (`test_connect_flow`), crates/tokio-nethernet/src/endpoint.rs (`client_delivers_answer_to_independent_subscribers`)
Verified by: `cargo test -p tokio-nethernet --all-features`; crates/jolyne/tests/nethernet_loopback.rs (`full_join_over_nethernet_loopback`) runs a full Bedrock join on top

### Requirement: Configuration bounds, deadlines and ICE servers
`NetherNetStreamConfig` SHALL be validated before any offer is created, rejecting out-of-range values with `NetherNetError::InvalidConfiguration`. Dialer and listener MUST honour the configured `connection_timeout` and `ice_servers`: the timeout covers credential lookup and the whole negotiation, and a non-empty `ice_servers` list replaces the signaling provider's credentials. No public STUN server is added implicitly.

#### Scenario: Rejected stream configuration
- **WHEN** `max_message_size` is 0 or above 262143, `incoming_channel_capacity` is 0 or above 1024, or `max_reassembly_size` is 0 or above 66846465 (255 × 262143)
- **THEN** `connect_with_config` and dialing/accepting with that config fail with `InvalidConfiguration`

#### Scenario: Defaults
- **WHEN** default configs are constructed
- **THEN** dialer and listener `connection_timeout` are 15 s, `ice_servers` is empty, stream `max_message_size` is 262143, `incoming_channel_capacity` is 64 and the reassembly timeout is 30 s

#### Scenario: Deadline includes credential wait
- **WHEN** the signaling provider never returns credentials and `connection_timeout` elapses
- **THEN** the dial fails with `NetherNetError::NegotiationTimeout` exactly at the configured timeout

#### Scenario: Configured ICE servers skip the provider
- **WHEN** `ice_servers` is non-empty and the provider's `credentials()` would block forever
- **THEN** negotiation proceeds without calling it

#### Scenario: Terminal signal on timeout
- **WHEN** a dial times out, or an accepted offer's data channels do not open within `connection_timeout`
- **THEN** the dialer sends `CONNECTERROR <id> 14` and the listener `CONNECTERROR <id> 15`, each bounded to 2 s

Evidence: crates/tokio-nethernet/src/stream.rs (`NetherNetStreamConfig::validate`, tests `rejects_invalid_configuration`, `configured_deadline_covers_credential_wait`, `configured_ice_servers_override_signaling_credential_wait`), crates/tokio-nethernet/src/dialer.rs (`ice_servers`, `perform_dial`), crates/tokio-nethernet/src/listener.rs (`handle_connect_request`), crates/tokio-nethernet/src/endpoint.rs (`negotiation_timeout_cleans_pending_and_notifies_listener`)

### Requirement: Signal text format
Signals SHALL use the text form `TYPE <connection id> <data>` with types `CONNECTREQUEST`, `CONNECTRESPONSE`, `CANDIDATEADD` and `CONNECTERROR`, split on the first two spaces. Parsing MUST follow go-nethernet: type names are case-sensitive, the connection ID is an unsigned decimal prefix that must not overflow `u64`, and `CONNECTERROR` data must start with a signed `i32` decimal. Error codes MUST be numbered 0..37 as in go-nethernet.

#### Scenario: Decimal prefixes accepted
- **WHEN** `CONNECTERROR 42junk -12suffix` or `CONNECTREQUEST 42junk offer data` is parsed
- **THEN** the connection ID is 42 and re-formatting the signal parses back to an equal value

#### Scenario: Malformed signals rejected
- **WHEN** a signal has an unknown or lower-case type, a missing second space, a signed or empty connection ID, an ID above `u64::MAX`, or an error code outside `i32` or without leading digits
- **THEN** `Signal::parse` and `Signal::parse_owned` return an error

#### Scenario: Identical wire text
- **WHEN** a signal is formatted with `Display`, `to_wire_string` or `write_wire`
- **THEN** all three produce identical text for every data length

Evidence: crates/tokio-nethernet/src/signaling.rs (`Signal::parse`, `split_wire`, `parse_signal_error_code`, `SignalErrorCode`, tests `upstream_signal_decimal_prefix_fixtures`, `signal_type_names_roundtrip_case_sensitively`, `wire_text_is_identical_across_inline_boundary`, `wire_head_formats_every_digit_count`, `test_signal_error_code_display`), crates/tokio-nethernet/PARITY.md (rows S1-S5)

### Requirement: Negotiation routing and peer-controlled bounds
Negotiations SHALL be keyed by (remote network ID, connection ID), so a signal from another network never reaches a pending negotiation. Peer-controlled state MUST be bounded: unknown-session signals and duplicate `CONNECTREQUEST`s are dropped, inline and trickled remote candidates share a 128-candidate budget, unparseable candidates are skipped without consuming budget, and a full per-negotiation queue fails that negotiation instead of blocking others.

#### Scenario: Foreign answer
- **WHEN** a pending dial to network `remote` receives a `CONNECTRESPONSE` with its connection ID from network `attacker`
- **THEN** the signal is not delivered to the dial

#### Scenario: Candidate budget
- **WHEN** a peer's inline and trickled candidates exceed 128 in total
- **THEN** adding the next one fails with `TooManyIceCandidates { limit: 128 }` and the budget never wraps
- **AND** a candidate that does not parse is ignored and the session continues

#### Scenario: Listener admission limits
- **WHEN** a `CONNECTREQUEST` arrives while the listener tracks `max_pending_connections` (default 64) negotiations or connections, or its SDP exceeds `max_sdp_size` (default 64 KiB)
- **THEN** the listener replies `CONNECTERROR` 16 or 17 respectively and creates no session
- **AND** a tracked entry is released only when its negotiation fails or its transport closes, and a stale cleanup never removes a replacement for the same key

#### Scenario: Dialer admission limits
- **WHEN** 64 dials are pending, or an incoming signal's data exceeds 1 MiB, or more than 32 candidates arrive before the answer
- **THEN** the new dial fails with `ConnectionClosed`, the oversized signal is dropped, and the dial fails with `InvalidPacket` respectively

#### Scenario: Remote cancellation
- **WHEN** a peer sends `CONNECTERROR` for a negotiation the listener is still completing
- **THEN** that connection is closed and no later timeout `CONNECTERROR` is sent for the same key

Evidence: crates/tokio-nethernet/src/dialer.rs (`CandidateBudget`, `add_candidate`, tests `rejects_signals_from_other_networks`, `inline_and_trickled_candidates_share_a_nonwrapping_budget`, `invalid_trickled_candidate_is_ignored`), crates/tokio-nethernet/src/listener.rs (tests `stale_cleanup_does_not_remove_replacement_connection`, `remote_cancellation_suppresses_pending_timeout_reply`), crates/tokio-nethernet/PARITY.md (rows D1, D3, N5)

### Requirement: SDP identity assertions
Dialers and listeners SHALL require a valid `a=identity` assertion from the peer unless the peer is explicitly allowed to omit it (`allow_identityless_server` / `allow_identityless_client`), or the signaling's `requires_identity()` is false and no token verifier is configured. A present assertion MUST always be verified: ES384 over the DTLS fingerprints with the token's `cpk` key. A listener that rejects a client identity MUST reply `CONNECTERROR <id> 37`.

#### Scenario: Server token policy
- **WHEN** a dialer receives an answer whose assertion is signed by the token's own `cpk`
- **THEN** it is accepted without temporal checks unless `verify_server_token` is set, which then replaces the self-signature check

#### Scenario: Client token policy
- **WHEN** a listener receives an offer assertion
- **THEN** `exp`/`iat`/`nbf` are checked with 60 s leeway and key possession is verified, but the JWT signature is not authenticated unless `verify_client_token` is set
- **AND** the verified key is exposed through `remote_identity().public_key` so Login can bind its `cpk`

#### Scenario: Tampering and ambiguity rejected
- **WHEN** the fingerprints, token claims or signing key do not match, the token is unsigned, or the SDP carries duplicate, conflicting or media-level identities
- **THEN** verification fails, and a malformed identity is never treated as absent

#### Scenario: Rejected client identity
- **WHEN** a listener that requires identities receives an offer without one
- **THEN** it replies `CONNECTERROR <id> 37` within 2 s and creates no session

#### Scenario: Default listener identity
- **WHEN** a listener whose signaling requires identities has no configured `identity`
- **THEN** it signs each answer with a fresh self-signed `"self"` token for one P-384 key generated per listener

Evidence: crates/tokio-nethernet/src/identity.rs (`verify_sdp`, `verify_sdp_optional`, `TokenVerifier`, tests `default_client_policy_rejects_expired_token_and_missing_identity`, `client_temporal_claims_use_go_sixty_second_leeway`, `fingerprint_tampering_is_rejected`, `wrong_token_key_and_unsigned_token_are_rejected`, `conflicting_fingerprints_and_duplicate_identity_are_rejected`, `optional_identity_only_allows_genuinely_absent_native_attribute`, `custom_server_verifier_can_reject_self_signed_token`, `externally_authenticated_client_binds_its_own_key`, `signed_server_sdp_round_trip_ignores_expired_token`), crates/tokio-nethernet/src/listener.rs (`rejected_client_identity_replies_identity_not_allowed`), crates/tokio-nethernet/PARITY.md (rows S6, S9, I1)

### Requirement: Session description and DTLS binding
Local SDP SHALL use go-nethernet's line order with `a=max-message-size:262144`, `sctp-port:5000` and the offerer as `actpass`. A remote description without `a=max-message-size` MUST mean 65536 and a present value MUST be a decimal above 1. The DTLS certificate MUST match the signaled fingerprint (sha-256, sha-384 or sha-512); a mismatch MUST fail the handshake rather than wait for the deadline.

#### Scenario: Missing max-message-size
- **WHEN** a remote description omits `a=max-message-size`
- **THEN** it parses with 65536, so outbound segments carry at most 65535 bytes

#### Scenario: Tampered fingerprint
- **WHEN** a signaling relay changes one digit of the offer's `a=fingerprint:sha-256`
- **THEN** the dial fails in under 4 s with a 5 s timeout configured, and the listener accepts no stream

#### Scenario: Weak fingerprint hashes
- **WHEN** a remote fingerprint uses a hash other than sha-256/384/512
- **THEN** parsing the description fails

Evidence: crates/tokio-nethernet/src/transport/sdp.rs (tests `encodes_go_nethernet_line_order_and_roundtrips`, `parse_enforces_parse_description_requirements`, `missing_max_message_size_defaults_to_libwebrtc_65536`, `dtls_roles_follow_answerer_role`), crates/tokio-nethernet/src/transport/certificate.rs (`fingerprint_algorithms_parse_case_insensitively_and_reject_weak_hashes`), crates/tokio-nethernet/src/stream.rs (`fingerprint_mismatch_rejects_the_connection`), crates/tokio-nethernet/tests/webrtc_rs_interop.rs (`dialer_connects_to_peer_preferring_aes_256_gcm_srtp`, `listener_accepts_independent_webrtc_rs_offerer`), crates/tokio-nethernet/PARITY.md (rows X8-X10)

### Requirement: Data channels and segment framing
The dialer SHALL open two DCEP channels, `ReliableDataChannel` (ordered, reliable) and `UnreliableDataChannel` (unordered, zero retransmits); `Message.reliable` selects the channel. Every data-channel message MUST be one segment: a count byte of segments still to follow, then payload of at most min(remote max-message-size − 1, `max_message_size`). Reliable messages MUST span at most 255 segments; unreliable ones exactly one, with count 0.

#### Scenario: Segmented reliable message
- **WHEN** a 140000-byte reliable message is sent with `max_message_size` 16383
- **THEN** the peer receives one 140000-byte message, and a single 262143-byte segment also round-trips

#### Scenario: Oversized messages rejected locally
- **WHEN** an unreliable message exceeds one segment, or a reliable one needs more than 255 segments
- **THEN** the send fails with `MessageTooLarge` or `TooManySegments` respectively, the stream stays usable, and nothing is written

#### Scenario: Empty message
- **WHEN** an empty buffer is sent
- **THEN** no segment is written and the send succeeds

Evidence: crates/tokio-nethernet/src/framing.rs (`segment_count`, `encode_segments`, tests `segment_count_never_wraps`, `encodes_countdown_headers_and_roundtrips_at_boundaries`), crates/tokio-nethernet/src/transport/session.rs (`RELIABLE_LABEL`, `UNRELIABLE_LABEL`, `send`), crates/tokio-nethernet/src/stream.rs (`real_loopback_fragmentation_unreliable_header_and_drained_close`), crates/tokio-nethernet/PARITY.md (rows X12, P1-P5, P8)

### Requirement: Reassembly validation and limits
The receiver SHALL validate every segment: an empty or header-only segment is `InvalidPacket`; a body above the local segment ceiling or a message above `max_reassembly_size` is `MessageTooLarge`; a reliable countdown gap or a non-zero unreliable count is `InvalidSegmentCount`; a first count of 255 is `TooManySegments`; and a continuation arriving after `reassembly_timeout` is `ReassemblyTimeout`. Any error MUST discard the partial message and reach the reader with its own variant.

#### Scenario: Exactly 255 segments
- **WHEN** a sender produces 255 segments counting down from 254
- **THEN** the message is reassembled, and a first count of 255 is rejected

#### Scenario: Countdown gap reaches the reader
- **WHEN** a peer sends reliable segments with counts 2 then 0
- **THEN** the reader's stream yields `InvalidSegmentCount { expected: 1, actual: 0 }`

Evidence: crates/tokio-nethernet/src/framing.rs (`Reassembler`, tests `receives_both_channel_headers_and_strict_countdown`, `receives_exactly_255_segments_and_rejects_256`, `receive_limits_include_single_packets_and_timeout`), crates/tokio-nethernet/src/stream.rs (`framing_errors_reach_the_reader_typed`), crates/tokio-nethernet/PARITY.md (rows P6, P7)

### Requirement: Send completion, backpressure, close and failure containment
A send SHALL complete once SCTP accepted every segment, never waiting for the peer's acknowledgement, and MUST pause while more than 1 MiB is unacknowledged. Inbound completed messages MUST be queued up to `incoming_channel_capacity`, after which SCTP reads pause. Graceful `close` MUST wait until queued bytes are acknowledged, then reset both channels and abort SCTP; dropping a stream aborts at once. A panic in the WebRTC stack or a hook MUST fail only the affected connection or offer.

#### Scenario: Sends do not wait for delayed SACK
- **WHEN** 20 small reliable messages are sent back to back while the peer reads nothing
- **THEN** all sends complete in under 200 ms and arrive in order

#### Scenario: Bounded inbound queue
- **WHEN** `incoming_channel_capacity` is 1 and 200 reliable messages are sent before the peer reads
- **THEN** every message arrives in order, with no message dropped

#### Scenario: Drained close
- **WHEN** a side sends a 140000-byte message and immediately closes
- **THEN** the peer receives the whole message, then `Err(ConnectionClosed)` within 1 s, then end of stream, and later sends fail with `ConnectionClosed`

#### Scenario: Contained panic
- **WHEN** a panic unwinds out of a transport call
- **THEN** that stream yields `DependencyPanic` once, then ends, its driver stops calling the stack, and the peer connection is unaffected
- **AND** a panic while a listener handles one offer replies `CONNECTERROR` 35 for that offer and the listener keeps serving others

Evidence: crates/tokio-nethernet/src/stream.rs (tests `small_reliable_sends_do_not_wait_for_delayed_sack`, `bounded_inbound_queue_backpressures_through_sctp_window`, `real_loopback_fragmentation_unreliable_header_and_drained_close`, `contained_panic_poisons_the_connection`), crates/tokio-nethernet/src/transport/session.rs (`BUFFERED_HIGH`, `BUFFERED_LOW`), crates/tokio-nethernet/src/error.rs (`catch_dependency_panic`), crates/tokio-nethernet/src/listener.rs (`negotiation_panic_fails_only_that_offer_with_typed_error`), crates/tokio-nethernet/PARITY.md (rows P9, X14, X15)

### Requirement: Local transport policy
`TransportConfig` SHALL choose which local addresses gather host candidates: by default every non-loopback, non-link-local UDP4/UDP6 interface address with remote `.local` candidates resolved query-only. `TransportConfig::loopback_v4()` MUST bind nothing but `127.0.0.1` (no mDNS socket). `relay_only` MUST advertise and use only TURN relay candidates. ICE server URLs other than `stun:` and UDP `turn:` MUST be skipped; hosts resolve concurrently within 3 s.

#### Scenario: Loopback confinement
- **WHEN** both peers use `loopback_v4()`
- **THEN** their offer and answer advertise only `127.0.0.1` candidates and the connection succeeds

#### Scenario: Relay-only through TURN
- **WHEN** a `relay_only` dialer is given a UDP `turn:` server, with candidates inline or trickled
- **THEN** it connects and exchanges messages entirely through the relay allocation
- **AND** a `stun:` and `turn:` URL for the same address gather srflx and relay together without timing out

#### Scenario: mDNS disabled
- **WHEN** `resolve_mdns` is false and a remote `.local` candidate arrives
- **THEN** it is ignored and no `0.0.0.0:5353` socket is bound

Evidence: crates/tokio-nethernet/src/transport/config.rs (`TransportConfig`, `default_policy_matches_pion_gathering`), crates/tokio-nethernet/src/transport/ice.rs (`resolve_servers`, tests `mdns_candidates_are_queried_and_resolved`, `disabled_mdns_ignores_local_candidates`), crates/tokio-nethernet/tests/loopback_settings.rs (`custom_settings_restrict_both_peers_to_ipv4_loopback`), crates/tokio-nethernet/tests/turn_relay.rs (`relay_only_dialer_exchanges_messages_through_turn`, `same_address_stun_and_turn_gather_without_timeout`, `trickled_relay_candidate_connects_relay_only_dialer`)

### Requirement: LAN discovery packet codec
The `discovery` packet codec SHALL encode request (0), response (1) and message (2) packets byte-for-byte like go-nethernet: an inclusive `u16` length, then HMAC-SHA256 and AES-256-ECB/PKCS7 under SHA-256 of LE `u64` `0xdeadbeef`. `decode` MUST verify the HMAC before parsing and reject truncated, corrupted, unknown-ID and over-declared packets without panicking. Server data MUST be version 7.

#### Scenario: Upstream vectors
- **WHEN** request, response and message packets are encoded
- **THEN** they equal go-nethernet's `Marshal` output, and decode back to the same packet and sender

#### Scenario: Every truncation rejected
- **WHEN** any prefix of a valid encoded message packet, or of a v7 server-data fixture, is decoded
- **THEN** decoding fails, as it does for server data with a trailing byte

#### Scenario: Response data past the declared length
- **WHEN** a response's hex data is longer than its declared length
- **THEN** the extra data is kept, as the client does

Evidence: crates/tokio-nethernet/src/discovery/packet.rs (`decode`, `ServerData::encode`, tests `codec_matches_go_nethernet_marshal`, `upstream_wire_regressions`, `malformed_packets_are_rejected_without_panics`, `server_data_v7_fixture_and_all_truncations`, `response_data_beyond_declared_length_is_kept`), crates/tokio-nethernet/src/discovery/crypto.rs (`test_key_is_sha256_of_deadbeef`, `midstate_hmac_matches_reference_hmac_at_every_padding_boundary`, `verify_hmac_rejects_a_flip_at_every_checksum_byte`), crates/tokio-nethernet/PARITY.md (rows L1-L6)
Verified by: `cargo test -p tokio-nethernet --features discovery`

### Requirement: LAN discovery listener behaviour
`DiscoveryListener` SHALL receive into a 65535-byte buffer and drop any datagram that fills it. Without server data it MUST send a request every `broadcast_interval` (2 s) to each interface's directed broadcast; while announcing it MUST only answer requests at their source. Peer addresses MUST be learned only from responses and messages, and peer state MUST be bounded. LAN negotiation MUST be non-trickle and MUST NOT require identities.

#### Scenario: Large SDP over UDP
- **WHEN** a message packet carrying a `CONNECTREQUEST` with an SDP of 256 candidate lines arrives after three truncated copies of it
- **THEN** the truncated copies are discarded and the full signal is delivered with the sender's network ID

#### Scenario: Request destinations
- **WHEN** the configured destination is `255.255.255.255:7551`
- **THEN** requests go to the directed broadcast of every non-loopback, non-169.254, non-/32 IPv4 network, deduplicated, plus `[ff02::1]` (and v4-mapped addresses) on an IPv6 socket, or the limited broadcast if no network qualifies

#### Scenario: Requests teach no route
- **WHEN** an announcing listener receives a request
- **THEN** it unicasts a response but cannot signal that peer until a message or response from it arrives

#### Scenario: Expiry
- **WHEN** a peer stays silent after a response
- **THEN** its server entry is dropped after 5 s and its address after 60 s

#### Scenario: Flood bounds
- **WHEN** 1200 distinct senders respond and 256 messages arrive with the 128-signal queue full
- **THEN** at most 1024 peers and responses are retained, new signals are dropped, reception never blocks, and an existing peer's response is still updated

#### Scenario: Inline candidates on LAN
- **WHEN** a LAN dialer and listener negotiate
- **THEN** offer and answer carry their candidates inline and no `CANDIDATEADD` is sent

Evidence: crates/tokio-nethernet/src/discovery/mod.rs (`DiscoveryListener`, `request_destinations`, tests `large_sdp_survives_udp_receive`, `bounded_state_and_full_signal_queue_do_not_block`, `recipient_filter_and_response_expiry`, `expiry_keeps_addresses_60s_and_servers_5s`, `requests_do_not_make_a_peer_addressable`, `directed_broadcasts_follow_interfaces`), crates/tokio-nethernet/tests/discovery_test.rs (`test_discovery_connect_flow`, `lan_offers_and_answers_carry_inline_candidates`, `announcing_listener_sends_no_requests`, `test_discovery_broadcast`), crates/tokio-nethernet/PARITY.md (rows L9-L17, N1, N7, S9)
Verified by: `cargo test -p tokio-nethernet --features discovery`

### Requirement: HTTP endpoint signaling
The `endpoint` feature SHALL implement go-nethernet's HTTP signaling: `GET`/`HEAD /v1/join` returns status JSON, and `POST /v1/join/<u64>` with an SDP body returns the answer as `200 application/sdp`. Bodies MUST be capped at 1 MiB and pending negotiations at 1024. The client MUST accept any 2xx answer, MUST treat an empty or all-digit body as a failure, and MUST reject endpoint URLs without an explicit port or with a path. Trickle ICE is disabled.

#### Scenario: Handler status codes
- **WHEN** an offer is over 1 MiB, empty, or addressed to a non-`u64` network ID, or the offer queue is full, or no answer arrives within 15 s
- **THEN** the handler replies 413, 400, 400, 503 and 502 respectively, leaves nothing pending, and after a timeout notifies the listener with `CONNECTERROR` 14
- **AND** other methods on `/v1/join` get 405 with an `Allow` header

#### Scenario: Client answer handling
- **WHEN** the endpoint replies 201 or 202 with an SDP body
- **THEN** the client delivers it as the answer to every subscriber, while a 300 status, an empty body, a body `37`, or a body over 1 MiB fails the offer

#### Scenario: Per-negotiation credentials
- **WHEN** a credential provider is configured
- **THEN** it is called for each negotiation and its errors fail that negotiation

Evidence: crates/tokio-nethernet/src/endpoint.rs (`Handler::handle_request`, `Client`, `parse_url`, tests `handler_serves_sdp_answer_and_cleans_pending`, `handler_rejects_bad_offers_and_full_admission_queue`, `negotiation_timeout_cleans_pending_and_notifies_listener`, `client_accepts_any_success_status`, `client_rejects_empty_numeric_and_oversized_answers`, `endpoint_urls_require_explicit_port_and_no_path`, `credentials_are_requested_each_time_and_errors_propagate`, `subscriptions_are_bounded_and_dropped_receivers_unregister`, `ping_method_mismatch_is_405`, `status_json_and_raknet_round_trip`), crates/tokio-nethernet/PARITY.md (rows H1, H2)
Verified by: `cargo test -p tokio-nethernet --features endpoint`

### Requirement: Franchise WebSocket signaling
The `xbox-signaling` feature SHALL provide `XboxSignaling::connect_with_url`, connecting with `Authorization` (the Minecraft service token), `Session-Id` and `Request-Id` headers and exchanging JSON envelopes `{Type, To|From, Message}` (0 heartbeat every 40 s, 1 signal, 2 credentials). Frames MUST be capped at 64 KiB and queues at 256. TURN credentials MUST come only from the service, never a peer, and expired ones MUST NOT be returned. It requires no identities.

#### Scenario: Credential frames from peers ignored
- **WHEN** a type-2 frame carries a `From` other than `Server`
- **THEN** it is ignored, while one without `From` or from `Server` replaces the shared credential cache

#### Scenario: Signal routing and recipient
- **WHEN** a signal is sent to network `"123"`
- **THEN** the envelope's `To` is the string `"123"` and its `Message` is the signal's wire text

#### Scenario: Credential refresh
- **WHEN** cached credentials expire (default lifetime 3600 s when the frame omits it)
- **THEN** `credentials()` waits for the next server credential frame, and fails once the socket has closed

#### Scenario: Rejected outgoing signals
- **WHEN** a `CONNECTERROR` has non-numeric data or a signal's data exceeds 64 KiB
- **THEN** sending fails without writing a frame

Evidence: crates/tokio-nethernet/src/xbox_signaling.rs (`XboxSignaling::connect_with_url`, tests `retained_socket_routes_signals_and_refreshes_shared_credentials`, `retained_signal_envelope_preserves_string_recipient`, `legacy_credentials_default_and_explicit_expiration`, `credentials_expire_and_wait_for_shared_refresh`, `malformed_or_oversized_signals_are_rejected`)
Verified by: `cargo test -p tokio-nethernet --features xbox-signaling`
